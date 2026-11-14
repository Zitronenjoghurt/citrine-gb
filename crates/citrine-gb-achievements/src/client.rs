use crate::game::{
    Achievement, Console, Event, GameState, Group, GroupKind, SignInError, Summary, User,
};
use crate::library::{GameProgress, SetInfo};
use rcheevos_sys as sys;
use std::collections::BTreeSet;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::ptr::NonNull;

pub trait Memory {
    fn read(&mut self, address: u32, buffer: &mut [u8]) -> usize;
}

pub struct NoMemory;

impl Memory for NoMemory {
    fn read(&mut self, _address: u32, _buffer: &mut [u8]) -> usize {
        0
    }
}

impl<F: FnMut(u32, &mut [u8]) -> usize> Memory for F {
    fn read(&mut self, address: u32, buffer: &mut [u8]) -> usize {
        self(address, buffer)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RequestId(u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub id: RequestId,
    pub url: String,
    pub post_data: Option<String>,
    pub content_type: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: i32,
    pub body: Vec<u8>,
}

impl Response {
    pub fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            body: body.into(),
        }
    }

    pub fn network_error() -> Self {
        Self {
            status: sys::RC_API_SERVER_RESPONSE_RETRYABLE_CLIENT_ERROR,
            body: Vec::new(),
        }
    }

    fn is_final(&self) -> bool {
        #[derive(serde::Deserialize)]
        struct Reply {
            #[serde(rename = "Success")]
            success: Option<bool>,
            #[serde(rename = "Error")]
            error: Option<String>,
        }
        let retry = match self.status {
            429 | 502 | 503 | 504 | 521..=525 => true,
            sys::RC_API_SERVER_RESPONSE_RETRYABLE_CLIENT_ERROR => true,
            sys::RC_API_SERVER_RESPONSE_CLIENT_ERROR => false,
            _ => self.body.first().is_none_or(|&byte| byte == 0),
        };
        if retry {
            return false;
        }
        serde_json::from_slice::<Reply>(&self.body)
            .is_ok_and(|reply| reply.success == Some(true) || reply.error.is_some())
    }
}

fn without_mastery(body: &[u8]) -> Option<Vec<u8>> {
    let mut reply: serde_json::Value = serde_json::from_slice(body).ok()?;
    let remaining = reply.get_mut("AchievementsRemaining")?;
    if remaining.as_u64() != Some(0) {
        return None;
    }
    *remaining = 1.into();
    serde_json::to_vec(&reply).ok()
}

fn awarded(post_data: &str) -> Option<u32> {
    let mut fields = post_data.split('&');
    if fields.next() != Some("r=awardachievement") {
        return None;
    }
    fields.find_map(|field| field.strip_prefix("a=")?.parse().ok())
}

struct Pending {
    id: RequestId,
    callback: sys::rc_client_server_callback_t,
    data: *mut c_void,
    award: Option<u32>,
}

#[derive(Default)]
struct Inner {
    memory: Option<*mut (dyn Memory + 'static)>,
    next_id: u64,
    outgoing: Vec<Request>,
    pending: Vec<Pending>,
    unsent: BTreeSet<u32>,
    events: Vec<Event>,
    runs_frames: bool,
}

const LOGIN_PASSWORD: usize = 1;
const LOGIN_TOKEN: usize = 2;
const LOAD_GAME: usize = 3;

pub struct Client {
    raw: NonNull<sys::rc_client_t>,
    inner: *mut Inner,
}

unsafe impl Send for Client {}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

impl Client {
    pub fn new() -> Self {
        let inner: *mut Inner = Box::into_raw(Box::default());
        let raw = unsafe { sys::rc_client_create(read_memory, server_call) };
        let raw = NonNull::new(raw).expect("rc_client_create returned null");
        unsafe {
            sys::rc_client_set_userdata(raw.as_ptr(), inner.cast());
            sys::rc_client_set_event_handler(raw.as_ptr(), event_handler);
            sys::rc_client_set_hardcore_enabled(raw.as_ptr(), 0);
            sys::rc_client_enable_logging(
                raw.as_ptr(),
                sys::RC_CLIENT_LOG_LEVEL_WARN,
                Some(log_message),
            );
        }
        Self { raw, inner }
    }

    fn raw(&self) -> *mut sys::rc_client_t {
        self.raw.as_ptr()
    }

    fn inner(&mut self) -> &mut Inner {
        unsafe { &mut *self.inner }
    }

    pub fn user_agent(&self, product: &str) -> String {
        let mut buffer = [0 as c_char; 64];
        let len =
            unsafe { sys::rc_client_get_user_agent_clause(self.raw(), buffer.as_mut_ptr(), 64) };
        if len == 0 {
            return product.to_owned();
        }
        format!("{product} {}", text(buffer.as_ptr()).unwrap_or_default())
    }

    pub fn set_host(&mut self, host: &str) {
        let host = CString::new(host).unwrap_or_default();
        unsafe { sys::rc_client_set_host(self.raw(), host.as_ptr()) };
    }

    pub fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.inner().outgoing)
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.inner().events)
    }

    pub fn waiting_on(&self) -> usize {
        unsafe { (*self.inner).pending.len() }
    }

    pub fn unsent_unlocks(&self) -> usize {
        unsafe { (*self.inner).unsent.len() }
    }

    pub fn complete(&mut self, id: RequestId, response: Response, memory: &mut dyn Memory) {
        let pending = &mut self.inner().pending;
        let Some(at) = pending.iter().position(|p| p.id == id) else {
            return;
        };
        let call = pending.remove(at);
        if let Some(award) = call.award
            && response.is_final()
        {
            self.inner().unsent.remove(&award);
        }
        let body = if call.award.is_some() && !self.has_game() {
            without_mastery(&response.body)
        } else {
            None
        };
        let body = body.as_deref().unwrap_or(&response.body);
        let start: &[u8] = if body.is_empty() { b"\0" } else { body };
        let raw_response = sys::rc_api_server_response_t {
            body: start.as_ptr().cast(),
            body_length: body.len(),
            http_status_code: response.status,
        };
        self.with_memory(memory, |_| unsafe {
            (call.callback)(&raw_response, call.data)
        });
    }

    pub fn sign_in_with_password(&mut self, username: &str, password: &str) {
        let username = CString::new(username).unwrap_or_default();
        let password = CString::new(password).unwrap_or_default();
        unsafe {
            sys::rc_client_begin_login_with_password(
                self.raw(),
                username.as_ptr(),
                password.as_ptr(),
                finished,
                LOGIN_PASSWORD as *mut c_void,
            )
        };
    }

    pub fn sign_in_with_token(&mut self, username: &str, token: &str) {
        let username = CString::new(username).unwrap_or_default();
        let token = CString::new(token).unwrap_or_default();
        unsafe {
            sys::rc_client_begin_login_with_token(
                self.raw(),
                username.as_ptr(),
                token.as_ptr(),
                finished,
                LOGIN_TOKEN as *mut c_void,
            )
        };
    }

    pub fn sign_out(&mut self) {
        self.inner().runs_frames = false;
        self.inner().unsent.clear();
        unsafe {
            sys::rc_client_unload_game(self.raw());
            sys::rc_client_logout(self.raw());
        }
    }

    pub fn user(&self) -> Option<User> {
        let user = unsafe { sys::rc_client_get_user_info(self.raw()).as_ref()? };
        Some(user_from(user))
    }

    pub fn load_game(&mut self, console: Console, rom: &[u8], memory: &mut dyn Memory) {
        self.inner().runs_frames = false;
        self.with_memory(memory, |raw| unsafe {
            sys::rc_client_begin_identify_and_load_game(
                raw,
                console.id(),
                c"rom".as_ptr(),
                rom.as_ptr(),
                rom.len(),
                finished,
                LOAD_GAME as *mut c_void,
            )
        });
    }

    pub fn unload_game(&mut self) {
        self.inner().runs_frames = false;
        unsafe { sys::rc_client_unload_game(self.raw()) };
    }

    pub fn runs_frames(&self) -> bool {
        unsafe { (*self.inner).runs_frames }
    }

    pub fn has_game(&self) -> bool {
        unsafe { sys::rc_client_is_game_loaded(self.raw()) != 0 }
    }

    pub fn do_frame(&mut self, memory: &mut dyn Memory) {
        if !self.runs_frames() {
            return;
        }
        self.with_memory(memory, |raw| unsafe { sys::rc_client_do_frame(raw) });
    }

    pub fn idle(&mut self, memory: &mut dyn Memory) {
        self.with_memory(memory, |raw| unsafe { sys::rc_client_idle(raw) });
    }

    pub fn reset(&mut self) {
        unsafe { sys::rc_client_reset(self.raw()) };
    }

    pub fn fetch_progress(&mut self, console: Console) {
        unsafe {
            sys::rc_client_begin_fetch_all_user_progress(
                self.raw(),
                console.id(),
                all_progress_fetched,
                console.id() as usize as *mut c_void,
            )
        };
    }

    pub fn fetch_sets(&mut self, console: Console) {
        unsafe {
            sys::rc_client_begin_fetch_game_list(
                self.raw(),
                console.id(),
                game_list_fetched,
                console.id() as usize as *mut c_void,
            )
        };
    }

    pub fn game(&self) -> Option<GameState> {
        let raw = self.raw();
        if unsafe { sys::rc_client_is_game_loaded(raw) } == 0 {
            return None;
        }
        let game = unsafe { sys::rc_client_get_game_info(raw).as_ref()? };
        let mut summary = sys::rc_client_user_game_summary_t::default();
        unsafe { sys::rc_client_get_user_game_summary(raw, &mut summary) };
        let list = unsafe {
            sys::rc_client_create_achievement_list(
                raw,
                sys::RC_CLIENT_ACHIEVEMENT_CATEGORY_CORE,
                sys::RC_CLIENT_ACHIEVEMENT_LIST_GROUPING_PROGRESS,
            )
        };
        let mut groups = Vec::new();
        if let Some(entries) = unsafe { list.as_ref() } {
            for bucket in unsafe { slice(entries.buckets, entries.num_buckets) } {
                let Some(kind) = group_kind(bucket.bucket_type) else {
                    continue;
                };
                let achievements = unsafe { slice(bucket.achievements, bucket.num_achievements) }
                    .iter()
                    .filter_map(|&a| unsafe { a.as_ref() })
                    .map(achievement_from)
                    .collect();
                groups.push(Group { kind, achievements });
            }
            unsafe { sys::rc_client_destroy_achievement_list(list) };
        }
        Some(GameState {
            id: game.id,
            title: text(game.title).unwrap_or_default(),
            hash: text(game.hash).unwrap_or_default(),
            badge_url: text(game.badge_url),
            summary: Summary {
                total: summary.num_core_achievements,
                unlocked: summary.num_unlocked_achievements,
                points_total: summary.points_core,
                points_unlocked: summary.points_unlocked,
            },
            groups,
        })
    }

    fn with_memory<R>(
        &mut self,
        memory: &mut dyn Memory,
        f: impl FnOnce(*mut sys::rc_client_t) -> R,
    ) -> R {
        struct Clear(*mut Inner);
        impl Drop for Clear {
            fn drop(&mut self) {
                unsafe { (*self.0).memory = None };
            }
        }
        let memory: *mut (dyn Memory + '_) = memory;
        let memory: *mut (dyn Memory + 'static) = unsafe { std::mem::transmute(memory) };
        unsafe { (*self.inner).memory = Some(memory) };
        let _clear = Clear(self.inner);
        f(self.raw())
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        unsafe {
            sys::rc_client_destroy(self.raw());
            drop(Box::from_raw(self.inner));
        }
    }
}

pub fn hash_rom(console: Console, rom: &[u8]) -> Option<String> {
    let mut hash = [0 as c_char; 33];
    let ok = unsafe {
        sys::rcheevos_sys_hash_buffer(hash.as_mut_ptr(), console.id(), rom.as_ptr(), rom.len())
    };
    if ok == 0 {
        return None;
    }
    text(hash.as_ptr())
}

unsafe fn inner_of(client: *const sys::rc_client_t) -> *mut Inner {
    unsafe { sys::rc_client_get_userdata(client).cast() }
}

unsafe fn slice<'a, T>(ptr: *const T, len: u32) -> &'a [T] {
    if ptr.is_null() || len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(ptr, len as usize) }
    }
}

fn text(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    (!text.is_empty()).then_some(text)
}

fn chars(bytes: &[c_char]) -> String {
    let bytes: Vec<u8> = bytes
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c.to_ne_bytes()[0])
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn user_from(user: &sys::rc_client_user_t) -> User {
    User {
        username: text(user.username).unwrap_or_default(),
        display_name: text(user.display_name)
            .or_else(|| text(user.username))
            .unwrap_or_default(),
        token: text(user.token).unwrap_or_default(),
        points: user.score_softcore,
        points_hardcore: user.score,
        avatar_url: text(user.avatar_url),
    }
}

fn achievement_from(a: &sys::rc_client_achievement_t) -> Achievement {
    let measured = chars(&a.measured_progress);
    let unlock_time: sys::time_t = a.unlock_time;
    Achievement {
        id: a.id,
        title: text(a.title).unwrap_or_default(),
        description: text(a.description).unwrap_or_default(),
        points: a.points,
        unlocked: a.unlocked & sys::RC_CLIENT_ACHIEVEMENT_UNLOCKED_SOFTCORE != 0,
        badge_name: chars(&a.badge_name),
        badge_url: text(a.badge_url),
        badge_locked_url: text(a.badge_locked_url),
        progress: (!measured.is_empty()).then_some((measured, a.measured_percent)),
        unlock_time: unlock_time as i64,
        rarity: a.rarity,
    }
}

fn group_kind(bucket: u8) -> Option<GroupKind> {
    Some(match bucket {
        sys::RC_CLIENT_ACHIEVEMENT_BUCKET_LOCKED => GroupKind::Locked,
        sys::RC_CLIENT_ACHIEVEMENT_BUCKET_UNLOCKED => GroupKind::Unlocked,
        sys::RC_CLIENT_ACHIEVEMENT_BUCKET_UNSUPPORTED => GroupKind::Unsupported,
        sys::RC_CLIENT_ACHIEVEMENT_BUCKET_UNOFFICIAL => GroupKind::Unofficial,
        sys::RC_CLIENT_ACHIEVEMENT_BUCKET_RECENTLY_UNLOCKED => GroupKind::RecentlyUnlocked,
        sys::RC_CLIENT_ACHIEVEMENT_BUCKET_ACTIVE_CHALLENGE => GroupKind::ActiveChallenge,
        sys::RC_CLIENT_ACHIEVEMENT_BUCKET_ALMOST_THERE => GroupKind::AlmostThere,
        sys::RC_CLIENT_ACHIEVEMENT_BUCKET_UNSYNCED => GroupKind::Unsynced,
        _ => return None,
    })
}

fn sign_in_error(result: c_int) -> SignInError {
    match result {
        sys::RC_INVALID_CREDENTIALS | sys::RC_ACCESS_DENIED => SignInError::WrongPassword,
        sys::RC_EXPIRED_TOKEN => SignInError::TokenExpired,
        sys::RC_NO_RESPONSE => SignInError::Network,
        _ => SignInError::Other,
    }
}

unsafe extern "C" fn read_memory(
    address: u32,
    buffer: *mut u8,
    num_bytes: u32,
    client: *mut sys::rc_client_t,
) -> u32 {
    let inner = unsafe { inner_of(client) };
    let Some(memory) = (unsafe { (*inner).memory }) else {
        return 0;
    };
    let buffer = unsafe { std::slice::from_raw_parts_mut(buffer, num_bytes as usize) };
    unsafe { (*memory).read(address, buffer) as u32 }
}

unsafe extern "C" fn server_call(
    request: *const sys::rc_api_request_t,
    callback: sys::rc_client_server_callback_t,
    callback_data: *mut c_void,
    client: *mut sys::rc_client_t,
) {
    let inner = unsafe { &mut *inner_of(client) };
    let request = unsafe { &*request };
    inner.next_id += 1;
    let id = RequestId(inner.next_id);
    let post_data = text(request.post_data);
    let award = post_data.as_deref().and_then(awarded);
    if let Some(award) = award {
        inner.unsent.insert(award);
    }
    inner.outgoing.push(Request {
        id,
        url: text(request.url).unwrap_or_default(),
        post_data,
        content_type: text(request.content_type),
    });
    inner.pending.push(Pending {
        id,
        callback,
        data: callback_data,
        award,
    });
}

unsafe extern "C" fn finished(
    result: c_int,
    error_message: *const c_char,
    client: *mut sys::rc_client_t,
    userdata: *mut c_void,
) {
    let inner = unsafe { &mut *inner_of(client) };
    let message = text(error_message).unwrap_or_default();
    let event = match userdata as usize {
        LOGIN_PASSWORD | LOGIN_TOKEN => {
            match unsafe { sys::rc_client_get_user_info(client).as_ref() } {
                Some(user) if result == sys::RC_OK => Event::SignedIn(user_from(user)),
                _ => Event::SignInFailed {
                    error: sign_in_error(result),
                    message,
                },
            }
        }
        LOAD_GAME => match result {
            sys::RC_OK => {
                inner.runs_frames = unsafe { sys::rc_client_is_processing_required(client) } != 0;
                Event::GameLoaded
            }
            sys::RC_NO_GAME_LOADED | sys::RC_NOT_FOUND => Event::NoSet,
            sys::RC_ABORTED | sys::RC_LOGIN_REQUIRED => return,
            _ => Event::GameLoadFailed(message),
        },
        _ => return,
    };
    inner.events.push(event);
}

unsafe extern "C" fn event_handler(
    event: *const sys::rc_client_event_t,
    client: *mut sys::rc_client_t,
) {
    let inner = unsafe { &mut *inner_of(client) };
    let event = unsafe { &*event };
    let event = match event.type_ {
        sys::RC_CLIENT_EVENT_ACHIEVEMENT_TRIGGERED => match unsafe { event.achievement.as_ref() } {
            Some(a) => Event::Unlocked(achievement_from(a)),
            None => return,
        },
        sys::RC_CLIENT_EVENT_GAME_COMPLETED => Event::GameCompleted,
        sys::RC_CLIENT_EVENT_SERVER_ERROR => Event::ServerError(
            unsafe { event.server_error.as_ref() }
                .and_then(|e| text(e.error_message))
                .unwrap_or_default(),
        ),
        sys::RC_CLIENT_EVENT_DISCONNECTED => Event::Disconnected,
        sys::RC_CLIENT_EVENT_RECONNECTED => Event::Reconnected,
        sys::RC_CLIENT_EVENT_RESET => Event::ResetNeeded,
        _ => return,
    };
    inner.events.push(event);
}

unsafe extern "C" fn log_message(message: *const c_char, _client: *const sys::rc_client_t) {
    if let Some(message) = text(message) {
        log::warn!("rcheevos: {message}");
    }
}

unsafe extern "C" fn all_progress_fetched(
    result: c_int,
    error_message: *const c_char,
    list: *mut sys::rc_client_all_user_progress_t,
    client: *mut sys::rc_client_t,
    userdata: *mut c_void,
) {
    let inner = unsafe { &mut *inner_of(client) };
    let Some(console) = Console::from_id(userdata as usize as u32) else {
        return;
    };
    let Some(list_ref) = (unsafe { list.as_ref() }).filter(|_| result == sys::RC_OK) else {
        inner.events.push(Event::FetchFailed {
            console,
            message: text(error_message).unwrap_or_default(),
        });
        return;
    };
    let games = unsafe { slice(list_ref.entries, list_ref.num_entries) }
        .iter()
        .map(|e| GameProgress {
            game_id: e.game_id,
            total: e.num_achievements,
            unlocked: e.num_unlocked_achievements,
            unlocked_hardcore: e.num_unlocked_achievements_hardcore,
        })
        .collect();
    unsafe { sys::rc_client_destroy_all_user_progress(list) };
    inner.events.push(Event::AllProgress { console, games });
}

unsafe extern "C" fn game_list_fetched(
    result: c_int,
    error_message: *const c_char,
    list: *mut sys::rc_client_game_list_t,
    client: *mut sys::rc_client_t,
    userdata: *mut c_void,
) {
    let inner = unsafe { &mut *inner_of(client) };
    let Some(console) = Console::from_id(userdata as usize as u32) else {
        return;
    };
    let Some(list_ref) = (unsafe { list.as_ref() }).filter(|_| result == sys::RC_OK) else {
        inner.events.push(Event::FetchFailed {
            console,
            message: text(error_message).unwrap_or_default(),
        });
        return;
    };
    let sets = unsafe { slice(list_ref.entries, list_ref.num_entries) }
        .iter()
        .map(|e| SetInfo {
            game_id: e.id,
            title: text(e.name).unwrap_or_default(),
            total: e.num_achievements,
            points: e.points,
            hashes: unsafe { slice(e.supported_hashes, e.num_supported_hashes) }
                .iter()
                .filter_map(|&h| text(h))
                .collect(),
        })
        .collect();
    unsafe { sys::rc_client_destroy_game_list(list) };
    inner.events.push(Event::GameList { console, sets });
}

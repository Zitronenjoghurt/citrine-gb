use crate::game::{
    Achievement, Attempt, Console, Entries, Entry, Event, Fetch, GameState, Group, GroupKind,
    Leaderboard, Load, ScoreFormat, Scoreboard, SignInError, Summary, User,
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
    load: Load,
    next_fetch: usize,
    fetches: Vec<(usize, u32, Fetch)>,
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
        self.inner().load = Load::Idle;
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
        self.inner().load = Load::Loading;
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
        self.inner().load = Load::Idle;
    }

    pub fn load(&self) -> &Load {
        unsafe { &(*self.inner).load }
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

    pub fn set_hardcore(&mut self, on: bool) {
        if self.hardcore() != on {
            unsafe { sys::rc_client_set_hardcore_enabled(self.raw(), c_int::from(on)) };
        }
    }

    pub fn hardcore(&self) -> bool {
        unsafe { sys::rc_client_get_hardcore_enabled(self.raw()) != 0 }
    }

    pub fn can_pause(&mut self) -> Result<(), u32> {
        let mut frames = 0;
        match unsafe { sys::rc_client_can_pause(self.raw(), &mut frames) } {
            0 => Err(frames.max(1)),
            _ => Ok(()),
        }
    }

    pub fn progress(&mut self) -> Option<Vec<u8>> {
        if !self.has_game() {
            return None;
        }
        let size = unsafe { sys::rc_client_progress_size(self.raw()) };
        let mut bytes = vec![0; size];
        let result = unsafe {
            sys::rc_client_serialize_progress_sized(self.raw(), bytes.as_mut_ptr(), bytes.len())
        };
        (result == sys::RC_OK).then_some(bytes)
    }

    pub fn restore_progress(&mut self, progress: Option<&[u8]>) {
        if !self.has_game() {
            return;
        }
        let (ptr, len) =
            progress.map_or((std::ptr::null(), 0), |bytes| (bytes.as_ptr(), bytes.len()));
        let result = unsafe { sys::rc_client_deserialize_progress_sized(self.raw(), ptr, len) };
        if result != sys::RC_OK {
            log::warn!("achievements: saved progress didn't load ({result}), starting it over");
            unsafe { sys::rc_client_deserialize_progress_sized(self.raw(), std::ptr::null(), 0) };
        }
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
        let unlock_bit = unlock_bit(self.hardcore());
        if let Some(entries) = unsafe { list.as_ref() } {
            for bucket in unsafe { slice(entries.buckets, entries.num_buckets) } {
                let Some(kind) = group_kind(bucket.bucket_type) else {
                    continue;
                };
                let achievements = unsafe { slice(bucket.achievements, bucket.num_achievements) }
                    .iter()
                    .filter_map(|&a| unsafe { a.as_ref() })
                    .map(|a| achievement_from(a, unlock_bit))
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
            leaderboards: leaderboards(raw),
        })
    }

    pub fn rich_presence(&mut self) -> Option<String> {
        let raw = self.raw();
        if unsafe { sys::rc_client_has_rich_presence(raw) } == 0 {
            return None;
        }
        let mut buffer = [0 as c_char; 256];
        let len = unsafe {
            sys::rc_client_get_rich_presence_message(raw, buffer.as_mut_ptr(), buffer.len())
        };
        if len == 0 {
            return None;
        }
        Some(chars(&buffer))
    }

    pub fn attempts(&self) -> Vec<Attempt> {
        let raw = self.raw();
        if unsafe { sys::rc_client_is_game_loaded(raw) } == 0 {
            return Vec::new();
        }
        let list = unsafe {
            sys::rc_client_create_leaderboard_list(
                raw,
                sys::RC_CLIENT_LEADERBOARD_LIST_GROUPING_NONE,
            )
        };
        let Some(entries) = (unsafe { list.as_ref() }) else {
            return Vec::new();
        };
        let attempts = unsafe { slice(entries.buckets, entries.num_buckets) }
            .iter()
            .flat_map(|bucket| unsafe { slice(bucket.leaderboards, bucket.num_leaderboards) })
            .filter_map(|&board| unsafe { board.as_ref() })
            .filter(|board| board.state == sys::RC_CLIENT_LEADERBOARD_STATE_TRACKING)
            .map(|board| Attempt {
                board: board.id,
                title: text(board.title).unwrap_or_default(),
                score: text(board.tracker_value).unwrap_or_default(),
            })
            .collect();
        unsafe { sys::rc_client_destroy_leaderboard_list(list) };
        attempts
    }

    pub fn fetch_entries(&mut self, board: u32, fetch: Fetch, count: u32) {
        let inner = self.inner();
        inner.next_fetch += 1;
        let key = inner.next_fetch;
        inner.fetches.push((key, board, fetch));
        let raw = self.raw();
        let data = key as *mut c_void;
        unsafe {
            match fetch {
                Fetch::Top => sys::rc_client_begin_fetch_leaderboard_entries(
                    raw,
                    board,
                    1,
                    count,
                    entries_fetched,
                    data,
                ),
                Fetch::AroundMe => sys::rc_client_begin_fetch_leaderboard_entries_around_user(
                    raw,
                    board,
                    count,
                    entries_fetched,
                    data,
                ),
            }
        };
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

fn unlock_bit(hardcore: bool) -> u8 {
    if hardcore {
        sys::RC_CLIENT_ACHIEVEMENT_UNLOCKED_HARDCORE
    } else {
        sys::RC_CLIENT_ACHIEVEMENT_UNLOCKED_SOFTCORE
    }
}

fn achievement_from(a: &sys::rc_client_achievement_t, unlock_bit: u8) -> Achievement {
    let measured = chars(&a.measured_progress);
    let unlock_time: sys::time_t = a.unlock_time;
    Achievement {
        id: a.id,
        title: text(a.title).unwrap_or_default(),
        description: text(a.description).unwrap_or_default(),
        points: a.points,
        unlocked: a.unlocked & unlock_bit != 0,
        badge_name: chars(&a.badge_name),
        badge_url: text(a.badge_url),
        badge_locked_url: text(a.badge_locked_url),
        progress: (!measured.is_empty()).then_some((measured, a.measured_percent)),
        unlock_time: unlock_time as i64,
        rarity: a.rarity,
    }
}

fn leaderboards(raw: *mut sys::rc_client_t) -> Vec<Leaderboard> {
    let list = unsafe {
        sys::rc_client_create_leaderboard_list(raw, sys::RC_CLIENT_LEADERBOARD_LIST_GROUPING_NONE)
    };
    let Some(entries) = (unsafe { list.as_ref() }) else {
        return Vec::new();
    };
    let boards = unsafe { slice(entries.buckets, entries.num_buckets) }
        .iter()
        .filter(|bucket| bucket.bucket_type != sys::RC_CLIENT_LEADERBOARD_BUCKET_UNSUPPORTED)
        .flat_map(|bucket| unsafe { slice(bucket.leaderboards, bucket.num_leaderboards) })
        .filter_map(|&board| unsafe { board.as_ref() })
        .map(leaderboard_from)
        .collect();
    unsafe { sys::rc_client_destroy_leaderboard_list(list) };
    boards
}

fn leaderboard_from(board: &sys::rc_client_leaderboard_t) -> Leaderboard {
    Leaderboard {
        id: board.id,
        title: text(board.title).unwrap_or_default(),
        description: text(board.description).unwrap_or_default(),
        format: match board.format {
            sys::RC_CLIENT_LEADERBOARD_FORMAT_TIME => ScoreFormat::Time,
            sys::RC_CLIENT_LEADERBOARD_FORMAT_SCORE => ScoreFormat::Score,
            _ => ScoreFormat::Value,
        },
        lower_is_better: board.lower_is_better != 0,
        tracking: board.state == sys::RC_CLIENT_LEADERBOARD_STATE_TRACKING,
    }
}

fn scoreboard_from(board: &sys::rc_client_leaderboard_scoreboard_t) -> Scoreboard {
    Scoreboard {
        board: board.leaderboard_id,
        submitted: chars(&board.submitted_score),
        best: chars(&board.best_score),
        rank: board.new_rank,
        entries: board.num_entries,
        top: unsafe { slice(board.top_entries, board.num_top_entries) }
            .iter()
            .map(|entry| Entry {
                rank: entry.rank,
                user: text(entry.username).unwrap_or_default(),
                score: chars(&entry.score),
            })
            .collect(),
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
        LOAD_GAME => {
            inner.load = match result {
                sys::RC_OK => {
                    inner.runs_frames =
                        unsafe { sys::rc_client_is_processing_required(client) } != 0;
                    inner.events.push(Event::GameLoaded);
                    Load::Loaded
                }
                sys::RC_NO_GAME_LOADED => Load::UnknownGame,
                sys::RC_NOT_FOUND => Load::NoSet,
                sys::RC_NO_RESPONSE => Load::Unreachable,
                sys::RC_ABORTED => return,
                sys::RC_LOGIN_REQUIRED => Load::Idle,
                _ => Load::Failed(message),
            };
            return;
        }
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
            Some(a) => {
                let hardcore = unsafe { sys::rc_client_get_hardcore_enabled(client) } != 0;
                Event::Unlocked(achievement_from(a, unlock_bit(hardcore)))
            }
            None => return,
        },
        sys::RC_CLIENT_EVENT_GAME_COMPLETED => Event::GameCompleted,
        sys::RC_CLIENT_EVENT_LEADERBOARD_STARTED
        | sys::RC_CLIENT_EVENT_LEADERBOARD_FAILED
        | sys::RC_CLIENT_EVENT_LEADERBOARD_SUBMITTED => {
            let Some(board) = (unsafe { event.leaderboard.as_ref() }) else {
                return;
            };
            let score = text(board.tracker_value).unwrap_or_default();
            let board = leaderboard_from(board);
            match event.type_ {
                sys::RC_CLIENT_EVENT_LEADERBOARD_STARTED => Event::BoardStarted(board),
                sys::RC_CLIENT_EVENT_LEADERBOARD_FAILED => Event::BoardFailed(board),
                _ => Event::BoardSubmitted { board, score },
            }
        }
        sys::RC_CLIENT_EVENT_LEADERBOARD_TRACKER_SHOW
        | sys::RC_CLIENT_EVENT_LEADERBOARD_TRACKER_UPDATE
        | sys::RC_CLIENT_EVENT_LEADERBOARD_TRACKER_HIDE => {
            if inner.events.last() == Some(&Event::TrackersChanged) {
                return;
            }
            Event::TrackersChanged
        }
        sys::RC_CLIENT_EVENT_LEADERBOARD_SCOREBOARD => {
            match unsafe { event.leaderboard_scoreboard.as_ref() } {
                Some(board) => Event::Scoreboard(scoreboard_from(board)),
                None => return,
            }
        }
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

unsafe extern "C" fn entries_fetched(
    result: c_int,
    error_message: *const c_char,
    list: *mut sys::rc_client_leaderboard_entry_list_t,
    client: *mut sys::rc_client_t,
    userdata: *mut c_void,
) {
    let inner = unsafe { &mut *inner_of(client) };
    let key = userdata as usize;
    let Some(at) = inner.fetches.iter().position(|&(k, _, _)| k == key) else {
        if !list.is_null() {
            unsafe { sys::rc_client_destroy_leaderboard_entry_list(list) };
        }
        return;
    };
    let (_, board, fetch) = inner.fetches.remove(at);
    let Some(list_ref) = (unsafe { list.as_ref() }).filter(|_| result == sys::RC_OK) else {
        inner.events.push(Event::EntriesFailed {
            board,
            fetch,
            message: text(error_message).unwrap_or_default(),
        });
        return;
    };
    let entries = unsafe { slice(list_ref.entries, list_ref.num_entries) }
        .iter()
        .map(|e| Entry {
            rank: e.rank,
            user: text(e.user).unwrap_or_default(),
            score: chars(&e.display),
        })
        .collect();
    let mine = usize::try_from(list_ref.user_index).ok();
    let total = list_ref.total_entries;
    unsafe { sys::rc_client_destroy_leaderboard_entry_list(list) };
    inner.events.push(Event::Entries(Entries {
        board,
        fetch,
        total,
        entries,
        mine,
    }));
}

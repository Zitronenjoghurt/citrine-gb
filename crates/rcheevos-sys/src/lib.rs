#![cfg(not(target_arch = "wasm32"))]
#![allow(non_camel_case_types)]

use std::ffi::{c_char, c_int, c_void};

#[cfg(windows)]
pub type time_t = i64;
#[cfg(not(windows))]
pub type time_t = std::ffi::c_long;

pub const RC_OK: c_int = 0;
pub const RC_API_FAILURE: c_int = -27;
pub const RC_LOGIN_REQUIRED: c_int = -28;
pub const RC_NO_GAME_LOADED: c_int = -29;
pub const RC_ABORTED: c_int = -31;
pub const RC_NO_RESPONSE: c_int = -32;
pub const RC_ACCESS_DENIED: c_int = -33;
pub const RC_INVALID_CREDENTIALS: c_int = -34;
pub const RC_EXPIRED_TOKEN: c_int = -35;
pub const RC_NOT_FOUND: c_int = -39;

pub const RC_API_SERVER_RESPONSE_CLIENT_ERROR: c_int = -1;
pub const RC_API_SERVER_RESPONSE_RETRYABLE_CLIENT_ERROR: c_int = -2;

pub const RC_CONSOLE_GAMEBOY: u32 = 4;
pub const RC_CONSOLE_GAMEBOY_ADVANCE: u32 = 5;
pub const RC_CONSOLE_GAMEBOY_COLOR: u32 = 6;
pub const RC_CONSOLE_NINTENDO_DS: u32 = 18;

pub const RC_CLIENT_EVENT_ACHIEVEMENT_TRIGGERED: u32 = 1;
pub const RC_CLIENT_EVENT_ACHIEVEMENT_PROGRESS_INDICATOR_SHOW: u32 = 7;
pub const RC_CLIENT_EVENT_ACHIEVEMENT_PROGRESS_INDICATOR_HIDE: u32 = 8;
pub const RC_CLIENT_EVENT_ACHIEVEMENT_PROGRESS_INDICATOR_UPDATE: u32 = 9;
pub const RC_CLIENT_EVENT_RESET: u32 = 14;
pub const RC_CLIENT_EVENT_GAME_COMPLETED: u32 = 15;
pub const RC_CLIENT_EVENT_SERVER_ERROR: u32 = 16;
pub const RC_CLIENT_EVENT_DISCONNECTED: u32 = 17;
pub const RC_CLIENT_EVENT_RECONNECTED: u32 = 18;

pub const RC_CLIENT_ACHIEVEMENT_CATEGORY_CORE: c_int = 1;

pub const RC_CLIENT_ACHIEVEMENT_LIST_GROUPING_LOCK_STATE: c_int = 0;
pub const RC_CLIENT_ACHIEVEMENT_LIST_GROUPING_PROGRESS: c_int = 1;

pub const RC_CLIENT_ACHIEVEMENT_BUCKET_LOCKED: u8 = 1;
pub const RC_CLIENT_ACHIEVEMENT_BUCKET_UNLOCKED: u8 = 2;
pub const RC_CLIENT_ACHIEVEMENT_BUCKET_UNSUPPORTED: u8 = 3;
pub const RC_CLIENT_ACHIEVEMENT_BUCKET_UNOFFICIAL: u8 = 4;
pub const RC_CLIENT_ACHIEVEMENT_BUCKET_RECENTLY_UNLOCKED: u8 = 5;
pub const RC_CLIENT_ACHIEVEMENT_BUCKET_ACTIVE_CHALLENGE: u8 = 6;
pub const RC_CLIENT_ACHIEVEMENT_BUCKET_ALMOST_THERE: u8 = 7;
pub const RC_CLIENT_ACHIEVEMENT_BUCKET_UNSYNCED: u8 = 8;

pub const RC_CLIENT_ACHIEVEMENT_STATE_UNLOCKED: u8 = 2;

pub const RC_CLIENT_ACHIEVEMENT_UNLOCKED_SOFTCORE: u8 = 1;

pub const RC_CLIENT_LOG_LEVEL_WARN: c_int = 2;

#[repr(C)]
pub struct rc_client_t {
    _private: [u8; 0],
}

#[repr(C)]
pub struct rc_client_async_handle_t {
    _private: [u8; 0],
}

#[repr(C)]
pub struct rc_api_request_t {
    pub url: *const c_char,
    pub post_data: *const c_char,
    pub content_type: *const c_char,
}

#[repr(C)]
pub struct rc_api_server_response_t {
    pub body: *const c_char,
    pub body_length: usize,
    pub http_status_code: c_int,
}

#[repr(C)]
pub struct rc_client_user_t {
    pub display_name: *const c_char,
    pub username: *const c_char,
    pub token: *const c_char,
    pub score: u32,
    pub score_softcore: u32,
    pub num_unread_messages: u32,
    pub avatar_url: *const c_char,
    pub avatar_last_updated: time_t,
}

#[repr(C)]
#[derive(Default)]
pub struct rc_client_user_game_summary_t {
    pub num_core_achievements: u32,
    pub num_unofficial_achievements: u32,
    pub num_unlocked_achievements: u32,
    pub num_unsupported_achievements: u32,
    pub points_core: u32,
    pub points_unlocked: u32,
    pub beaten_time: time_t,
    pub completed_time: time_t,
}

#[repr(C)]
pub struct rc_client_game_t {
    pub id: u32,
    pub console_id: u32,
    pub title: *const c_char,
    pub hash: *const c_char,
    pub badge_name: *const c_char,
    pub badge_url: *const c_char,
}

#[repr(C)]
pub struct rc_client_achievement_t {
    pub title: *const c_char,
    pub description: *const c_char,
    pub badge_name: [c_char; 8],
    pub measured_progress: [c_char; 24],
    pub measured_percent: f32,
    pub id: u32,
    pub points: u32,
    pub unlock_time: time_t,
    pub state: u8,
    pub category: u8,
    pub bucket: u8,
    pub unlocked: u8,
    pub rarity: f32,
    pub rarity_hardcore: f32,
    pub type_: u8,
    pub badge_url: *const c_char,
    pub badge_locked_url: *const c_char,
}

#[repr(C)]
pub struct rc_client_achievement_bucket_t {
    pub achievements: *const *const rc_client_achievement_t,
    pub num_achievements: u32,
    pub label: *const c_char,
    pub subset_id: u32,
    pub bucket_type: u8,
}

#[repr(C)]
pub struct rc_client_achievement_list_t {
    pub buckets: *const rc_client_achievement_bucket_t,
    pub num_buckets: u32,
}

#[repr(C)]
pub struct rc_client_server_error_t {
    pub error_message: *const c_char,
    pub api: *const c_char,
    pub result: c_int,
    pub related_id: u32,
}

#[repr(C)]
pub struct rc_client_event_t {
    pub type_: u32,
    pub achievement: *mut rc_client_achievement_t,
    pub leaderboard: *mut c_void,
    pub leaderboard_tracker: *mut c_void,
    pub leaderboard_scoreboard: *mut c_void,
    pub server_error: *mut rc_client_server_error_t,
    pub subset: *mut c_void,
}

#[repr(C)]
pub struct rc_client_all_user_progress_entry_t {
    pub game_id: u32,
    pub num_achievements: u32,
    pub num_unlocked_achievements: u32,
    pub num_unlocked_achievements_hardcore: u32,
}

#[repr(C)]
pub struct rc_client_all_user_progress_t {
    pub entries: *mut rc_client_all_user_progress_entry_t,
    pub num_entries: u32,
}

#[repr(C)]
pub struct rc_client_hash_library_entry_t {
    pub hash: [c_char; 33],
    pub game_id: u32,
}

#[repr(C)]
pub struct rc_client_hash_library_t {
    pub entries: *mut rc_client_hash_library_entry_t,
    pub num_entries: u32,
}

#[repr(C)]
pub struct rc_client_game_list_entry_t {
    pub id: u32,
    pub num_achievements: u32,
    pub num_leaderboards: u32,
    pub points: u32,
    pub name: *const c_char,
    pub image_name: *const c_char,
    pub image_url: *const c_char,
    pub supported_hashes: *const *const c_char,
    pub unsupported_hashes: *const *const c_char,
    pub num_supported_hashes: u32,
    pub num_unsupported_hashes: u32,
}

#[repr(C)]
pub struct rc_client_game_list_t {
    pub entries: *mut rc_client_game_list_entry_t,
    pub num_entries: u32,
}

pub type rc_client_read_memory_func_t = unsafe extern "C" fn(
    address: u32,
    buffer: *mut u8,
    num_bytes: u32,
    client: *mut rc_client_t,
) -> u32;
pub type rc_client_server_callback_t = unsafe extern "C" fn(
    server_response: *const rc_api_server_response_t,
    callback_data: *mut c_void,
);
pub type rc_client_server_call_t = unsafe extern "C" fn(
    request: *const rc_api_request_t,
    callback: rc_client_server_callback_t,
    callback_data: *mut c_void,
    client: *mut rc_client_t,
);
pub type rc_client_callback_t = unsafe extern "C" fn(
    result: c_int,
    error_message: *const c_char,
    client: *mut rc_client_t,
    userdata: *mut c_void,
);
pub type rc_client_message_callback_t =
    unsafe extern "C" fn(message: *const c_char, client: *const rc_client_t);
pub type rc_client_event_handler_t =
    unsafe extern "C" fn(event: *const rc_client_event_t, client: *mut rc_client_t);
pub type rc_get_time_millisecs_func_t = unsafe extern "C" fn(client: *const rc_client_t) -> u64;
pub type rc_client_fetch_all_user_progress_callback_t = unsafe extern "C" fn(
    result: c_int,
    error_message: *const c_char,
    list: *mut rc_client_all_user_progress_t,
    client: *mut rc_client_t,
    callback_userdata: *mut c_void,
);
pub type rc_client_fetch_hash_library_callback_t = unsafe extern "C" fn(
    result: c_int,
    error_message: *const c_char,
    list: *mut rc_client_hash_library_t,
    client: *mut rc_client_t,
    callback_userdata: *mut c_void,
);
pub type rc_client_fetch_game_list_callback_t = unsafe extern "C" fn(
    result: c_int,
    error_message: *const c_char,
    list: *mut rc_client_game_list_t,
    client: *mut rc_client_t,
    callback_userdata: *mut c_void,
);

unsafe extern "C" {
    pub fn rc_client_create(
        read_memory_function: rc_client_read_memory_func_t,
        server_call_function: rc_client_server_call_t,
    ) -> *mut rc_client_t;
    pub fn rc_client_destroy(client: *mut rc_client_t);
    pub fn rc_client_set_hardcore_enabled(client: *mut rc_client_t, enabled: c_int);
    pub fn rc_client_get_hardcore_enabled(client: *const rc_client_t) -> c_int;
    pub fn rc_client_set_userdata(client: *mut rc_client_t, userdata: *mut c_void);
    pub fn rc_client_get_userdata(client: *const rc_client_t) -> *mut c_void;
    pub fn rc_client_set_host(client: *mut rc_client_t, hostname: *const c_char);
    pub fn rc_client_set_get_time_millisecs_function(
        client: *mut rc_client_t,
        handler: rc_get_time_millisecs_func_t,
    );
    pub fn rc_client_abort_async(
        client: *mut rc_client_t,
        async_handle: *mut rc_client_async_handle_t,
    );
    pub fn rc_client_get_user_agent_clause(
        client: *mut rc_client_t,
        buffer: *mut c_char,
        buffer_size: usize,
    ) -> usize;
    pub fn rc_client_enable_logging(
        client: *mut rc_client_t,
        level: c_int,
        callback: Option<rc_client_message_callback_t>,
    );

    pub fn rc_client_begin_login_with_password(
        client: *mut rc_client_t,
        username: *const c_char,
        password: *const c_char,
        callback: rc_client_callback_t,
        callback_userdata: *mut c_void,
    ) -> *mut rc_client_async_handle_t;
    pub fn rc_client_begin_login_with_token(
        client: *mut rc_client_t,
        username: *const c_char,
        token: *const c_char,
        callback: rc_client_callback_t,
        callback_userdata: *mut c_void,
    ) -> *mut rc_client_async_handle_t;
    pub fn rc_client_logout(client: *mut rc_client_t);
    pub fn rc_client_get_user_info(client: *const rc_client_t) -> *const rc_client_user_t;
    pub fn rc_client_user_get_image_url(
        user: *const rc_client_user_t,
        buffer: *mut c_char,
        buffer_size: usize,
    ) -> c_int;
    pub fn rc_client_get_user_game_summary(
        client: *const rc_client_t,
        summary: *mut rc_client_user_game_summary_t,
    );
    pub fn rc_client_begin_fetch_all_user_progress(
        client: *mut rc_client_t,
        console_id: u32,
        callback: rc_client_fetch_all_user_progress_callback_t,
        callback_userdata: *mut c_void,
    ) -> *mut rc_client_async_handle_t;
    pub fn rc_client_destroy_all_user_progress(list: *mut rc_client_all_user_progress_t);
    pub fn rc_client_begin_fetch_hash_library(
        client: *mut rc_client_t,
        console_id: u32,
        callback: rc_client_fetch_hash_library_callback_t,
        callback_userdata: *mut c_void,
    ) -> *mut rc_client_async_handle_t;
    pub fn rc_client_destroy_hash_library(list: *mut rc_client_hash_library_t);
    pub fn rc_client_begin_fetch_game_list(
        client: *mut rc_client_t,
        console_id: u32,
        callback: rc_client_fetch_game_list_callback_t,
        callback_userdata: *mut c_void,
    ) -> *mut rc_client_async_handle_t;
    pub fn rc_client_destroy_game_list(list: *mut rc_client_game_list_t);

    pub fn rc_client_begin_identify_and_load_game(
        client: *mut rc_client_t,
        console_id: u32,
        file_path: *const c_char,
        data: *const u8,
        data_size: usize,
        callback: rc_client_callback_t,
        callback_userdata: *mut c_void,
    ) -> *mut rc_client_async_handle_t;
    pub fn rc_client_begin_load_game(
        client: *mut rc_client_t,
        hash: *const c_char,
        callback: rc_client_callback_t,
        callback_userdata: *mut c_void,
    ) -> *mut rc_client_async_handle_t;
    pub fn rc_client_get_load_game_state(client: *const rc_client_t) -> c_int;
    pub fn rc_client_is_game_loaded(client: *const rc_client_t) -> c_int;
    pub fn rc_client_unload_game(client: *mut rc_client_t);
    pub fn rc_client_get_game_info(client: *const rc_client_t) -> *const rc_client_game_t;

    pub fn rc_client_get_achievement_info(
        client: *mut rc_client_t,
        id: u32,
    ) -> *const rc_client_achievement_t;
    pub fn rc_client_achievement_get_image_url(
        achievement: *const rc_client_achievement_t,
        state: c_int,
        buffer: *mut c_char,
        buffer_size: usize,
    ) -> c_int;
    pub fn rc_client_create_achievement_list(
        client: *mut rc_client_t,
        category: c_int,
        grouping: c_int,
    ) -> *mut rc_client_achievement_list_t;
    pub fn rc_client_destroy_achievement_list(list: *mut rc_client_achievement_list_t);
    pub fn rc_client_has_achievements(client: *mut rc_client_t) -> c_int;

    pub fn rc_client_set_event_handler(
        client: *mut rc_client_t,
        handler: rc_client_event_handler_t,
    );
    pub fn rc_client_set_read_memory_function(
        client: *mut rc_client_t,
        handler: rc_client_read_memory_func_t,
    );
    pub fn rc_client_is_processing_required(client: *mut rc_client_t) -> c_int;
    pub fn rc_client_do_frame(client: *mut rc_client_t);
    pub fn rc_client_idle(client: *mut rc_client_t);
    pub fn rc_client_can_pause(client: *mut rc_client_t, frames_remaining: *mut u32) -> c_int;
    pub fn rc_client_reset(client: *mut rc_client_t);
    pub fn rc_client_progress_size(client: *mut rc_client_t) -> usize;
    pub fn rc_client_serialize_progress_sized(
        client: *mut rc_client_t,
        buffer: *mut u8,
        buffer_size: usize,
    ) -> c_int;
    pub fn rc_client_deserialize_progress_sized(
        client: *mut rc_client_t,
        serialized: *const u8,
        serialized_size: usize,
    ) -> c_int;

    pub fn rcheevos_sys_hash_buffer(
        hash: *mut c_char,
        console_id: u32,
        buffer: *const u8,
        buffer_size: usize,
    ) -> c_int;
    pub fn rcheevos_sys_layout(which: c_int) -> usize;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    fn c(which: c_int) -> usize {
        unsafe { rcheevos_sys_layout(which) }
    }

    #[test]
    fn struct_sizes_match_c() {
        assert_eq!(size_of::<rc_api_server_response_t>(), c(0));
        assert_eq!(size_of::<rc_client_user_t>(), c(1));
        assert_eq!(size_of::<rc_client_user_game_summary_t>(), c(2));
        assert_eq!(size_of::<rc_client_game_t>(), c(3));
        assert_eq!(size_of::<rc_client_achievement_t>(), c(4));
        assert_eq!(size_of::<rc_client_achievement_bucket_t>(), c(5));
        assert_eq!(size_of::<rc_client_achievement_list_t>(), c(6));
        assert_eq!(size_of::<rc_client_server_error_t>(), c(7));
        assert_eq!(size_of::<rc_client_event_t>(), c(8));
        assert_eq!(size_of::<rc_client_all_user_progress_entry_t>(), c(9));
        assert_eq!(size_of::<rc_client_hash_library_entry_t>(), c(10));
        assert_eq!(size_of::<rc_client_game_list_entry_t>(), c(11));
    }

    #[test]
    fn field_offsets_match_c() {
        assert_eq!(offset_of!(rc_api_request_t, content_type), c(100));
        assert_eq!(offset_of!(rc_client_user_t, avatar_url), c(101));
        assert_eq!(offset_of!(rc_client_achievement_t, unlock_time), c(102));
        assert_eq!(offset_of!(rc_client_achievement_t, type_), c(103));
        assert_eq!(
            offset_of!(rc_client_achievement_t, badge_locked_url),
            c(104)
        );
        assert_eq!(
            offset_of!(rc_client_achievement_bucket_t, bucket_type),
            c(105)
        );
        assert_eq!(offset_of!(rc_client_event_t, server_error), c(106));
        assert_eq!(
            offset_of!(rc_client_user_game_summary_t, completed_time),
            c(107)
        );
        assert_eq!(offset_of!(rc_client_hash_library_entry_t, game_id), c(108));
        assert_eq!(
            offset_of!(rc_api_server_response_t, http_status_code),
            c(109)
        );
        assert_eq!(
            offset_of!(rc_client_game_list_entry_t, num_unsupported_hashes),
            c(110)
        );
    }
}

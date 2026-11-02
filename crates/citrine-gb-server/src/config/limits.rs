use super::{ConfigError, number};
use std::time::Duration;

pub const VARS: &[&str] = &[
    "CITRINE_QUEUE_MB",
    "CITRINE_SEND_TIMEOUT_SECS",
    "CITRINE_DOWNLOADS_AT_ONCE",
    "CITRINE_DOWNLOADS_WAITING",
    "CITRINE_STORAGE_TOTAL_MB",
    "CITRINE_DISK_FLOOR_MB",
    "CITRINE_DISK_FLOOR_PERCENT",
    "CITRINE_MAX_FILES",
    "CITRINE_MAX_SAVES",
    "CITRINE_INBOX_MESSAGES",
    "CITRINE_INBOX_MB",
    "CITRINE_MAX_CONNECTIONS",
    "CITRINE_IP_CONNECTIONS",
    "CITRINE_IP_UNSIGNED",
    "CITRINE_IP_CONNECTS_PER_MINUTE",
    "CITRINE_SIGN_IN_FRAME_KB",
    "CITRINE_WRONG_TRIES_PER_MINUTE",
    "CITRINE_UNKNOWN_TRIES_PER_MINUTE",
    "CITRINE_MESSAGES_PER_SECOND",
    "CITRINE_MESSAGE_BURST",
    "CITRINE_CHATS_PER_SECOND",
    "CITRINE_CHAT_BURST",
    "CITRINE_CHAT_KB_PER_SECOND",
    "CITRINE_FRIEND_REQUESTS_PER_HOUR",
    "CITRINE_FRIEND_REQUEST_BURST",
    "CITRINE_DOWNLOAD_MB_PER_DAY",
    "CITRINE_SESSION_KB_PER_SECOND",
    "CITRINE_SESSION_BURST_MB",
    "CITRINE_MAX_INVITES",
    "CITRINE_PASSWORD_CHECKS",
    "CITRINE_PASSWORD_QUEUE",
    "CITRINE_SIGN_UPS_PER_IP",
    "CITRINE_SIGN_UPS_PER_HOUR",
    "CITRINE_FRIEND_REQUESTS_OUT",
    "CITRINE_FRIEND_REQUESTS_IN",
    "CITRINE_FRIEND_REQUEST_DAYS",
];

#[derive(Debug, Clone, PartialEq)]
pub struct Limits {
    pub queue_bytes: u32,
    pub send_timeout: Duration,
    pub downloads_at_once: usize,
    pub downloads_waiting: usize,
    pub storage_total_bytes: u64,
    pub disk_floor_bytes: u64,
    pub disk_floor_percent: u64,
    pub max_files: u64,
    pub max_saves: u64,
    pub inbox_messages: u64,
    pub inbox_bytes: u64,
    pub max_connections: usize,
    pub ip_connections: usize,
    pub ip_unsigned: usize,
    pub ip_connects_per_minute: u64,
    pub sign_in_frame_bytes: usize,
    pub wrong_tries_per_minute: u32,
    pub unknown_tries_per_minute: u32,
    pub messages_per_second: u64,
    pub message_burst: u64,
    pub chats_per_second: u64,
    pub chat_burst: u64,
    pub chat_bytes_per_second: u64,
    pub friend_requests_per_hour: u64,
    pub friend_request_burst: u64,
    pub download_bytes_per_day: u64,
    pub session_bytes_per_second: u64,
    pub session_burst_bytes: u64,
    pub max_invites: usize,
    pub password_checks: usize,
    pub password_queue: usize,
    pub sign_ups_per_ip: u64,
    pub sign_ups_per_hour: u64,
    pub friend_requests_out: u64,
    pub friend_requests_in: u64,
    pub friend_request_age: Duration,
}

const MB: u64 = 1 << 20;

impl Limits {
    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let get = |name: &'static str, default: u64| number(name, var(name), default);
        Ok(Self {
            queue_bytes: (get("CITRINE_QUEUE_MB", 8)?.max(1) * MB).min(u32::MAX as u64 / 2) as u32,
            send_timeout: Duration::from_secs(get("CITRINE_SEND_TIMEOUT_SECS", 20)?.max(1)),
            downloads_at_once: get("CITRINE_DOWNLOADS_AT_ONCE", 2)?.max(1) as usize,
            downloads_waiting: get("CITRINE_DOWNLOADS_WAITING", 16)? as usize,
            storage_total_bytes: get("CITRINE_STORAGE_TOTAL_MB", 10_240)?.saturating_mul(MB),
            disk_floor_bytes: get("CITRINE_DISK_FLOOR_MB", 2048)?.saturating_mul(MB),
            disk_floor_percent: get("CITRINE_DISK_FLOOR_PERCENT", 5)?.min(100),
            max_files: get("CITRINE_MAX_FILES", 1000)?,
            max_saves: get("CITRINE_MAX_SAVES", 500)?,
            inbox_messages: get("CITRINE_INBOX_MESSAGES", 2000)?,
            inbox_bytes: get("CITRINE_INBOX_MB", 32)?.saturating_mul(MB),
            max_connections: get("CITRINE_MAX_CONNECTIONS", 500)? as usize,
            ip_connections: get("CITRINE_IP_CONNECTIONS", 10)? as usize,
            ip_unsigned: get("CITRINE_IP_UNSIGNED", 3)? as usize,
            ip_connects_per_minute: get("CITRINE_IP_CONNECTS_PER_MINUTE", 20)?,
            sign_in_frame_bytes: (get("CITRINE_SIGN_IN_FRAME_KB", 4)?.max(1) << 10) as usize,
            wrong_tries_per_minute: get("CITRINE_WRONG_TRIES_PER_MINUTE", 10)?
                .clamp(1, u32::MAX as u64) as u32,
            unknown_tries_per_minute: get("CITRINE_UNKNOWN_TRIES_PER_MINUTE", 60)?
                .clamp(1, u32::MAX as u64) as u32,
            messages_per_second: get("CITRINE_MESSAGES_PER_SECOND", 30)?.max(1),
            message_burst: get("CITRINE_MESSAGE_BURST", 100)?.max(1),
            chats_per_second: get("CITRINE_CHATS_PER_SECOND", 5)?.max(1),
            chat_burst: get("CITRINE_CHAT_BURST", 30)?.max(1),
            chat_bytes_per_second: get("CITRINE_CHAT_KB_PER_SECOND", 256)?
                .max(1)
                .saturating_mul(1 << 10),
            friend_requests_per_hour: get("CITRINE_FRIEND_REQUESTS_PER_HOUR", 20)?.max(1),
            friend_request_burst: get("CITRINE_FRIEND_REQUEST_BURST", 10)?.max(1),
            download_bytes_per_day: get("CITRINE_DOWNLOAD_MB_PER_DAY", 1024)?
                .max(1)
                .saturating_mul(MB),
            session_bytes_per_second: get("CITRINE_SESSION_KB_PER_SECOND", 256)?
                .max(1)
                .saturating_mul(1 << 10),
            session_burst_bytes: get("CITRINE_SESSION_BURST_MB", 12)?
                .max(1)
                .saturating_mul(MB),
            max_invites: get("CITRINE_MAX_INVITES", 5)?.max(1) as usize,
            password_checks: get("CITRINE_PASSWORD_CHECKS", 2)?.clamp(1, 64) as usize,
            password_queue: get("CITRINE_PASSWORD_QUEUE", 16)?.min(4096) as usize,
            sign_ups_per_ip: get("CITRINE_SIGN_UPS_PER_IP", 3)?.max(1),
            sign_ups_per_hour: get("CITRINE_SIGN_UPS_PER_HOUR", 30)?.max(1),
            friend_requests_out: get("CITRINE_FRIEND_REQUESTS_OUT", 50)?.max(1),
            friend_requests_in: get("CITRINE_FRIEND_REQUESTS_IN", 200)?.max(1),
            friend_request_age: Duration::from_secs(
                get("CITRINE_FRIEND_REQUEST_DAYS", 30)?
                    .max(1)
                    .saturating_mul(86_400),
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn limits(pairs: &[(&str, &str)]) -> Limits {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Limits::from_vars(|name| map.get(name).cloned()).unwrap()
    }

    #[test]
    fn memory_defaults() {
        let l = limits(&[]);
        assert_eq!(l.queue_bytes, 8 << 20);
        assert_eq!(l.send_timeout, Duration::from_secs(20));
        assert_eq!(l.downloads_at_once, 2);
        assert_eq!(l.downloads_waiting, 16);
    }

    #[test]
    fn disk_defaults() {
        let l = limits(&[]);
        assert_eq!(l.storage_total_bytes, 10 << 30);
        assert_eq!(l.disk_floor_bytes, 2 << 30);
        assert_eq!(l.disk_floor_percent, 5);
        assert_eq!(l.max_files, 1000);
        assert_eq!(l.max_saves, 500);
        assert_eq!(l.inbox_messages, 2000);
        assert_eq!(l.inbox_bytes, 32 << 20);
    }

    #[test]
    fn disk_vars_are_read() {
        let l = limits(&[
            ("CITRINE_STORAGE_TOTAL_MB", "3"),
            ("CITRINE_DISK_FLOOR_MB", "4"),
            ("CITRINE_DISK_FLOOR_PERCENT", "250"),
            ("CITRINE_MAX_FILES", "5"),
            ("CITRINE_MAX_SAVES", "6"),
            ("CITRINE_INBOX_MESSAGES", "7"),
            ("CITRINE_INBOX_MB", "8"),
        ]);
        assert_eq!(l.storage_total_bytes, 3 << 20);
        assert_eq!(l.disk_floor_bytes, 4 << 20);
        assert_eq!(l.disk_floor_percent, 100);
        assert_eq!(l.max_files, 5);
        assert_eq!(l.max_saves, 6);
        assert_eq!(l.inbox_messages, 7);
        assert_eq!(l.inbox_bytes, 8 << 20);
    }

    #[test]
    fn connection_defaults() {
        let l = limits(&[]);
        assert_eq!(l.max_connections, 500);
        assert_eq!(l.ip_connections, 10);
        assert_eq!(l.ip_unsigned, 3);
        assert_eq!(l.ip_connects_per_minute, 20);
        assert_eq!(l.sign_in_frame_bytes, 4096);
    }

    #[test]
    fn lockout_defaults_and_vars() {
        let l = limits(&[]);
        assert_eq!(l.wrong_tries_per_minute, 10);
        assert_eq!(l.unknown_tries_per_minute, 60);
        let l = limits(&[
            ("CITRINE_WRONG_TRIES_PER_MINUTE", "3"),
            ("CITRINE_UNKNOWN_TRIES_PER_MINUTE", "0"),
        ]);
        assert_eq!(l.wrong_tries_per_minute, 3);
        assert_eq!(l.unknown_tries_per_minute, 1);
    }

    #[test]
    fn rate_defaults_and_vars() {
        let l = limits(&[]);
        assert_eq!((l.messages_per_second, l.message_burst), (30, 100));
        assert_eq!((l.chats_per_second, l.chat_burst), (5, 30));
        assert_eq!(l.chat_bytes_per_second, 256 << 10);
        assert_eq!(
            (l.friend_requests_per_hour, l.friend_request_burst),
            (20, 10)
        );
        assert_eq!(l.download_bytes_per_day, 1 << 30);
        assert_eq!(l.session_bytes_per_second, 256 << 10);
        assert_eq!(l.session_burst_bytes, 12 << 20);
        assert_eq!(l.max_invites, 5);
        assert_eq!((l.password_checks, l.password_queue), (2, 16));
        let l = limits(&[
            ("CITRINE_CHAT_BURST", "3"),
            ("CITRINE_DOWNLOAD_MB_PER_DAY", "2"),
            ("CITRINE_PASSWORD_CHECKS", "0"),
            ("CITRINE_PASSWORD_QUEUE", "0"),
        ]);
        assert_eq!(l.chat_burst, 3);
        assert_eq!(l.download_bytes_per_day, 2 << 20);
        assert_eq!((l.password_checks, l.password_queue), (1, 0));
    }

    #[test]
    fn sign_up_defaults_and_vars() {
        let l = limits(&[]);
        assert_eq!((l.sign_ups_per_ip, l.sign_ups_per_hour), (3, 30));
        let l = limits(&[
            ("CITRINE_SIGN_UPS_PER_IP", "5"),
            ("CITRINE_SIGN_UPS_PER_HOUR", "0"),
        ]);
        assert_eq!((l.sign_ups_per_ip, l.sign_ups_per_hour), (5, 1));
    }

    #[test]
    fn friend_request_defaults_and_vars() {
        let l = limits(&[]);
        assert_eq!((l.friend_requests_out, l.friend_requests_in), (50, 200));
        assert_eq!(l.friend_request_age, Duration::from_secs(30 * 86_400));
        let l = limits(&[("CITRINE_FRIEND_REQUEST_DAYS", "2")]);
        assert_eq!(l.friend_request_age, Duration::from_secs(2 * 86_400));
    }

    #[test]
    fn memory_vars_are_read() {
        let l = limits(&[
            ("CITRINE_QUEUE_MB", "2"),
            ("CITRINE_SEND_TIMEOUT_SECS", "3"),
            ("CITRINE_DOWNLOADS_AT_ONCE", "0"),
            ("CITRINE_DOWNLOADS_WAITING", "0"),
        ]);
        assert_eq!(l.queue_bytes, 2 << 20);
        assert_eq!(l.send_timeout, Duration::from_secs(3));
        assert_eq!(l.downloads_at_once, 1);
        assert_eq!(l.downloads_waiting, 0);
    }
}

#[cfg(not(target_os = "android"))]
use crate::online::push::StatusLine;
use citrine_gb_net::{OnlineId, SessionId};

#[cfg(target_os = "android")]
pub use super::android::push::{
    allowed, ask, available, keep_status_line, open_settings, take_opened, with_token,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(any(target_os = "android", test)),
    expect(dead_code, reason = "only Android reports opened notifications")
)]
pub enum Opened {
    Friend(OnlineId),
    Invite { from: OnlineId, session: SessionId },
    Chat(OnlineId),
}

#[cfg_attr(
    not(any(target_os = "android", test)),
    expect(dead_code, reason = "only Android reports opened notifications")
)]
impl Opened {
    pub fn parse(kind: &str, from: &str, session: &str) -> Option<Self> {
        let from = from.parse().ok()?;
        match kind {
            "friend_request" => Some(Self::Friend(from)),
            "invite" => Some(Self::Invite {
                from,
                session: session.parse().ok()?,
            }),
            "chat" => Some(Self::Chat(from)),
            _ => None,
        }
    }
}

#[cfg(not(target_os = "android"))]
pub fn available() -> bool {
    false
}

#[cfg(not(target_os = "android"))]
pub fn allowed() -> bool {
    false
}

#[cfg(not(target_os = "android"))]
pub fn ask() {}

#[cfg(not(target_os = "android"))]
pub fn open_settings() {}

#[cfg(not(target_os = "android"))]
pub fn with_token<R>(read: impl FnOnce(Option<&str>) -> R) -> R {
    read(None)
}

#[cfg(not(target_os = "android"))]
pub fn take_opened() -> Option<Opened> {
    None
}

#[cfg(not(target_os = "android"))]
pub fn keep_status_line(_line: Option<StatusLine>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tap_opens_what_the_notification_was_about() {
        let ana = OnlineId::new(42).unwrap();
        let id = ana.to_string();
        assert_eq!(
            Opened::parse("friend_request", &id, ""),
            Some(Opened::Friend(ana))
        );
        assert_eq!(Opened::parse("chat", &id, ""), Some(Opened::Chat(ana)));
        assert_eq!(
            Opened::parse("invite", &id, "99"),
            Some(Opened::Invite {
                from: ana,
                session: 99
            })
        );
        assert_eq!(Opened::parse("invite", &id, ""), None);
        assert_eq!(Opened::parse("chat", "nobody", ""), None);
        assert_eq!(Opened::parse("news", &id, ""), None);
    }
}

mod fcm;

use crate::App;
use crate::db::{Db, PushTarget};
use citrine_gb_net::{OnlineId, PushKinds, SessionId};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc;

pub use fcm::DEFAULT_ENDPOINT;

const INVITE_TTL: &str = "300s";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    FriendRequest { from: i64 },
    Invite { from: i64, session: SessionId },
    Chat { from: i64 },
}

impl Event {
    fn from(self) -> i64 {
        match self {
            Self::FriendRequest { from } | Self::Invite { from, .. } | Self::Chat { from } => from,
        }
    }

    fn wanted(self, kinds: PushKinds) -> bool {
        match self {
            Self::FriendRequest { .. } => kinds.friend_requests,
            Self::Invite { .. } => kinds.invites,
            Self::Chat { .. } => kinds.chats,
        }
    }
}

pub struct Sender {
    online_id: OnlineId,
    name: String,
}

pub fn message(token: &str, event: Event, sender: &Sender, unread: u32) -> Value {
    let from = sender.online_id.to_string();
    let mut data = json!({ "from": from, "name": sender.name });
    let mut android = json!({ "priority": "HIGH" });
    match event {
        Event::FriendRequest { .. } => data["kind"] = "friend_request".into(),
        Event::Invite { session, .. } => {
            data["kind"] = "invite".into();
            data["session"] = session.to_string().into();
            android["ttl"] = INVITE_TTL.into();
        }
        Event::Chat { .. } => {
            data["kind"] = "chat".into();
            data["count"] = unread.max(1).to_string().into();
            android["collapse_key"] = format!("chat-{from}").into();
        }
    }
    json!({ "message": { "token": token, "data": data, "android": android } })
}

pub struct Push {
    jobs: Option<mpsc::UnboundedSender<(i64, Event)>>,
}

impl Push {
    pub fn off() -> Self {
        Self { jobs: None }
    }

    pub fn start(key_file: Option<&Path>, endpoint: &str, db: Arc<Db>) -> Self {
        let Some(path) = key_file else {
            log::info!("push notifications are off: CITRINE_FCM_KEY_FILE is not set");
            return Self::off();
        };
        let fcm = match fcm::Fcm::from_file(path, endpoint) {
            Ok(fcm) => fcm,
            Err(err) => {
                log::error!(
                    "push notifications are off: {} can't be used: {err}",
                    path.display()
                );
                return Self::off();
            }
        };
        log::info!(
            "push notifications go through Firebase project {}",
            fcm.project()
        );
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(run(fcm, db, rx));
        Self { jobs: Some(tx) }
    }

    pub fn send(&self, user: i64, event: Event) {
        if let Some(jobs) = &self.jobs {
            let _ = jobs.send((user, event));
        }
    }
}

impl App {
    pub(crate) fn notify(&self, user: i64, event: Event) {
        if !self.hub.active(user) {
            self.push.send(user, event);
        }
    }

    pub(crate) async fn rings(&self, user: i64, event: Event) -> bool {
        self.push.jobs.is_some()
            && self
                .db
                .push_targets(user)
                .await
                .is_ok_and(|targets| targets.iter().any(|t| event.wanted(t.kinds)))
    }
}

async fn run(mut fcm: fcm::Fcm, db: Arc<Db>, mut jobs: mpsc::UnboundedReceiver<(i64, Event)>) {
    while let Some((user, event)) = jobs.recv().await {
        if let Err(err) = deliver(&mut fcm, &db, user, event).await {
            log::warn!("push notification not sent: {err}");
        }
    }
}

async fn deliver(fcm: &mut fcm::Fcm, db: &Db, user: i64, event: Event) -> sqlx::Result<()> {
    let targets: Vec<PushTarget> = db
        .push_targets(user)
        .await?
        .into_iter()
        .filter(|t| event.wanted(t.kinds))
        .collect();
    if targets.is_empty() {
        return Ok(());
    }
    let Some(row) = db.account(event.from()).await? else {
        return Ok(());
    };
    let sender = Sender {
        online_id: row.online_id,
        name: row.display_name,
    };
    for target in targets {
        let unread = match event {
            Event::Chat { from } => db.unread_from(target.device, from).await?,
            _ => 0,
        };
        let body = message(&target.token, event, &sender, unread);
        if fcm.send(&body).await == fcm::Outcome::Gone {
            db.drop_push_token(&target.token).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ana() -> Sender {
        Sender {
            online_id: OnlineId::new(42).unwrap(),
            name: "Ana".into(),
        }
    }

    #[test]
    fn a_chat_collapses_into_one_notification_per_friend() {
        let body = message("tok", Event::Chat { from: 3 }, &ana(), 3);
        let from = OnlineId::new(42).unwrap().to_string();
        assert_eq!(
            body,
            json!({ "message": {
                "token": "tok",
                "data": { "kind": "chat", "from": from, "name": "Ana", "count": "3" },
                "android": { "priority": "HIGH", "collapse_key": format!("chat-{from}") },
            }})
        );
    }

    #[test]
    fn an_invite_expires_with_the_invite() {
        let body = message(
            "tok",
            Event::Invite {
                from: 3,
                session: 99,
            },
            &ana(),
            0,
        );
        assert_eq!(body["message"]["data"]["kind"], "invite");
        assert_eq!(body["message"]["data"]["session"], "99");
        assert_eq!(body["message"]["android"]["ttl"], INVITE_TTL);
        assert!(body["message"]["android"].get("collapse_key").is_none());
    }

    #[test]
    fn a_friend_request_names_who_sent_it() {
        let body = message("tok", Event::FriendRequest { from: 3 }, &ana(), 0);
        assert_eq!(body["message"]["data"]["kind"], "friend_request");
        assert_eq!(body["message"]["data"]["name"], "Ana");
        assert!(body["message"]["data"].get("count").is_none());
    }

    #[test]
    fn each_kind_has_its_own_switch() {
        let only = |friend_requests, invites, chats| PushKinds {
            friend_requests,
            invites,
            chats,
        };
        let request = Event::FriendRequest { from: 1 };
        let invite = Event::Invite {
            from: 1,
            session: 2,
        };
        let chat = Event::Chat { from: 1 };
        assert!(request.wanted(only(true, false, false)));
        assert!(!request.wanted(only(false, true, true)));
        assert!(invite.wanted(only(false, true, false)));
        assert!(!invite.wanted(only(true, false, true)));
        assert!(chat.wanted(only(false, false, true)));
        assert!(!chat.wanted(only(true, true, false)));
    }
}

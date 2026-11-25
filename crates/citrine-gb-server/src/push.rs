mod fcm;

use crate::App;
use crate::db::{Db, PushTarget};
use citrine_gb_net::{OnlineId, PushKinds, SessionId};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

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

    fn what(self) -> &'static str {
        match self {
            Self::FriendRequest { .. } => "a friend request",
            Self::Invite { .. } => "an invite",
            Self::Chat { .. } => "a message",
        }
    }

    fn kind(self) -> &'static str {
        match self {
            Self::FriendRequest { .. } => "friend requests",
            Self::Invite { .. } => "invites",
            Self::Chat { .. } => "messages",
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

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Delivery {
    phones: usize,
    wanted: usize,
    sent: usize,
    gone: usize,
    failed: Vec<String>,
}

pub fn report(event: Event, delivery: &Delivery) -> String {
    let head = format!("push notification for {}", event.what());
    if delivery.phones == 0 {
        return format!("{head} not sent: the account has no phone registered for notifications");
    }
    if delivery.wanted == 0 {
        let phones = match delivery.phones {
            1 => "1 phone is".to_string(),
            n => format!("{n} phones are"),
        };
        return format!(
            "{head} not sent: {phones} registered, with notifications for {} off",
            event.kind()
        );
    }
    let mut text = format!(
        "{head} went to {} of {} phones",
        delivery.sent, delivery.wanted
    );
    if delivery.gone > 0 {
        let was = if delivery.gone == 1 { "was" } else { "were" };
        text += &format!(
            "; {} {was} unknown to Firebase and is dropped",
            delivery.gone
        );
    }
    for reason in &delivery.failed {
        text += &format!("; {reason}");
    }
    text
}

pub fn registered(kinds: PushKinds) -> String {
    let taken: Vec<&str> = [
        (kinds.friend_requests, "friend requests"),
        (kinds.invites, "invites"),
        (kinds.chats, "messages"),
    ]
    .into_iter()
    .filter_map(|(on, name)| on.then_some(name))
    .collect();
    let list = match taken.as_slice() {
        [] => {
            return "push notifications: a phone registered with every kind off (its notifications are off in Android, or every switch is off)".into();
        }
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    };
    format!("push notifications: a phone registered for {list}")
}

pub struct Sender {
    online_id: OnlineId,
    name: String,
}

pub fn message(token: &str, event: Event, sender: &Sender, unread: u32) -> Value {
    let from = sender.online_id.to_string();
    let mut data = json!({ "sender": from, "name": sender.name });
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
    task: Mutex<Option<JoinHandle<()>>>,
}

impl Push {
    pub fn off() -> Self {
        Self {
            jobs: None,
            task: Mutex::new(None),
        }
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
        Self {
            jobs: Some(tx),
            task: Mutex::new(Some(tokio::spawn(run(fcm, db, rx)))),
        }
    }

    pub async fn stop(&self) {
        let task = self.task.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(task) = task {
            task.abort();
            let _ = task.await;
        }
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
        match deliver(&mut fcm, &db, user, event).await {
            Ok(delivery) if delivery.failed.is_empty() => {
                log::info!("{}", report(event, &delivery));
            }
            Ok(delivery) => log::warn!("{}", report(event, &delivery)),
            Err(err) => log::warn!("push notification not sent: {err}"),
        }
    }
}

async fn deliver(fcm: &mut fcm::Fcm, db: &Db, user: i64, event: Event) -> sqlx::Result<Delivery> {
    let targets = db.push_targets(user).await?;
    let mut delivery = Delivery {
        phones: targets.len(),
        ..Delivery::default()
    };
    let targets: Vec<PushTarget> = targets
        .into_iter()
        .filter(|t| event.wanted(t.kinds))
        .collect();
    delivery.wanted = targets.len();
    if targets.is_empty() {
        return Ok(delivery);
    }
    let Some(row) = db.account(event.from()).await? else {
        return Ok(delivery);
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
        match fcm.send(&body).await {
            fcm::Outcome::Sent => delivery.sent += 1,
            fcm::Outcome::Gone => {
                delivery.gone += 1;
                db.drop_push_token(&target.token).await?;
            }
            fcm::Outcome::Failed(reason) => {
                if !delivery.failed.contains(&reason) {
                    delivery.failed.push(reason);
                }
            }
        }
    }
    Ok(delivery)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use axum::routing::post;
    use citrine_gb_net::PublicKey;
    use rsa::pkcs8::{EncodePrivateKey, LineEnding};
    use std::sync::OnceLock;

    const PROJECT: &str = "citrine-test";
    const MISMATCH: &str = "Firebase refused it: 403 Forbidden, SENDER_ID_MISMATCH: SenderId mismatch (the app on the phone is in another Firebase project than the server's key)";

    async fn fake_firebase() -> String {
        async fn token() -> String {
            json!({ "access_token": "fake", "expires_in": 3600 }).to_string()
        }
        async fn send(body: String) -> (StatusCode, String) {
            let body: Value = serde_json::from_str(&body).unwrap();
            let error = |status: StatusCode, name: &str, code: &str, message: &str| {
                let details = json!([{ "@type": "type.googleapis.com/google.firebase.fcm.v1.FcmError", "errorCode": code }]);
                let error = json!({ "error": {
                    "code": status.as_u16(), "message": message, "status": name, "details": details,
                }});
                (status, error.to_string())
            };
            if body["message"]["data"].get("from").is_some() {
                return error(
                    StatusCode::BAD_REQUEST,
                    "INVALID_ARGUMENT",
                    "INVALID_ARGUMENT",
                    "Invalid data payload key: from",
                );
            }
            match body["message"]["token"].as_str() {
                Some("gone") => error(
                    StatusCode::NOT_FOUND,
                    "NOT_FOUND",
                    "UNREGISTERED",
                    "Requested entity was not found.",
                ),
                Some("other-project") => error(
                    StatusCode::FORBIDDEN,
                    "PERMISSION_DENIED",
                    "SENDER_ID_MISMATCH",
                    "SenderId mismatch",
                ),
                _ => (
                    StatusCode::OK,
                    json!({ "name": "projects/p/messages/1" }).to_string(),
                ),
            }
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let router = axum::Router::new()
            .route("/token", post(token))
            .route(&format!("/v1/projects/{PROJECT}/messages:send"), post(send));
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        url
    }

    fn fake_fcm(url: &str) -> fcm::Fcm {
        static PEM: OnceLock<String> = OnceLock::new();
        let pem = PEM.get_or_init(|| {
            let key = rsa::RsaPrivateKey::new(&mut rand::thread_rng(), 2048).unwrap();
            key.to_pkcs8_pem(LineEnding::LF).unwrap().to_string()
        });
        let account = json!({
            "project_id": PROJECT,
            "client_email": "push@citrine-test.iam.gserviceaccount.com",
            "private_key": pem,
            "token_uri": format!("{url}/token"),
        });
        fcm::Fcm::from_json(&account.to_string(), url).unwrap()
    }

    #[tokio::test]
    async fn each_way_a_push_ends_is_counted() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("t.db")).await.unwrap();
        let ana = db
            .create_account(&[1; 32], &PublicKey([1; 32]), "Ana", 10)
            .await
            .unwrap()
            .id;
        let ben = db
            .create_account(&[2; 32], &PublicKey([2; 32]), "Ben", 10)
            .await
            .unwrap()
            .id;
        let mut fcm = fake_fcm(&fake_firebase().await);
        let chat = Event::Chat { from: ana };
        assert_eq!(
            deliver(&mut fcm, &db, ben, chat).await.unwrap(),
            Delivery::default()
        );
        let phone = db.add_device(ben, &[7; 32], "Pixel", 10).await.unwrap();
        db.set_push_token(ben, phone, "phone", PushKinds::default(), 11)
            .await
            .unwrap();
        assert_eq!(
            deliver(&mut fcm, &db, ben, chat).await.unwrap(),
            Delivery {
                phones: 1,
                ..Delivery::default()
            }
        );
        db.set_push_token(ben, phone, "phone", PushKinds::ALL, 12)
            .await
            .unwrap();
        let tablet = db.add_device(ben, &[8; 32], "Tab", 10).await.unwrap();
        db.set_push_token(ben, tablet, "other-project", PushKinds::ALL, 12)
            .await
            .unwrap();
        let old = db.add_device(ben, &[9; 32], "Old", 10).await.unwrap();
        db.set_push_token(ben, old, "gone", PushKinds::ALL, 12)
            .await
            .unwrap();
        assert_eq!(
            deliver(&mut fcm, &db, ben, chat).await.unwrap(),
            Delivery {
                phones: 3,
                wanted: 3,
                sent: 1,
                gone: 1,
                failed: vec![MISMATCH.into()],
            }
        );
        let left: Vec<String> = db
            .push_targets(ben)
            .await
            .unwrap()
            .into_iter()
            .map(|t| t.token)
            .collect();
        assert_eq!(left, ["phone", "other-project"]);
    }

    #[test]
    fn the_log_line_says_why_nothing_arrived() {
        let chat = Event::Chat { from: 1 };
        assert_eq!(
            report(chat, &Delivery::default()),
            "push notification for a message not sent: the account has no phone registered for notifications"
        );
        assert_eq!(
            report(
                Event::FriendRequest { from: 1 },
                &Delivery {
                    phones: 2,
                    ..Delivery::default()
                }
            ),
            "push notification for a friend request not sent: 2 phones are registered, with notifications for friend requests off"
        );
        assert_eq!(
            report(
                Event::Invite {
                    from: 1,
                    session: 2
                },
                &Delivery {
                    phones: 1,
                    wanted: 1,
                    sent: 1,
                    ..Delivery::default()
                }
            ),
            "push notification for an invite went to 1 of 1 phones"
        );
        assert_eq!(
            report(
                chat,
                &Delivery {
                    phones: 3,
                    wanted: 3,
                    sent: 1,
                    gone: 1,
                    failed: vec![MISMATCH.into()],
                }
            ),
            format!(
                "push notification for a message went to 1 of 3 phones; 1 was unknown to Firebase and is dropped; {MISMATCH}"
            )
        );
    }

    #[test]
    fn a_registration_names_what_the_phone_takes() {
        let kinds = |friend_requests, invites, chats| PushKinds {
            friend_requests,
            invites,
            chats,
        };
        assert_eq!(
            registered(PushKinds::ALL),
            "push notifications: a phone registered for friend requests, invites and messages"
        );
        assert_eq!(
            registered(kinds(true, false, true)),
            "push notifications: a phone registered for friend requests and messages"
        );
        assert_eq!(
            registered(kinds(false, false, true)),
            "push notifications: a phone registered for messages"
        );
        assert_eq!(
            registered(PushKinds::default()),
            "push notifications: a phone registered with every kind off (its notifications are off in Android, or every switch is off)"
        );
    }

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
                "data": { "kind": "chat", "sender": from, "name": "Ana", "count": "3" },
                "android": { "priority": "HIGH", "collapse_key": format!("chat-{from}") },
            }})
        );
    }

    #[test]
    fn no_data_key_is_one_firebase_keeps_for_itself() {
        let events = [
            Event::FriendRequest { from: 3 },
            Event::Invite {
                from: 3,
                session: 99,
            },
            Event::Chat { from: 3 },
        ];
        for event in events {
            let body = message("tok", event, &ana(), 2);
            let data = body["message"]["data"].as_object().unwrap();
            for key in data.keys() {
                assert!(
                    key != "from"
                        && key != "message_type"
                        && !key.starts_with("google")
                        && !key.starts_with("gcm"),
                    "{event:?} sends the reserved data key {key:?}"
                );
            }
        }
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

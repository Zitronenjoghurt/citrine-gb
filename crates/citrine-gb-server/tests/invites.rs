use citrine_gb_net::{
    ClientMsg, ErrorCode, InviteCode, PROTOCOL_VERSION, RelayId, RelayPassword, ServerInfo,
    ServerMsg,
};
use citrine_gb_server::{Config, Running, start};
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

const PASSWORD: &str = "lemon tree";
const ADMIN: &str = "admin pass";

async fn server(dir: &Path) -> Running {
    let map: HashMap<String, String> = HashMap::from([
        ("CITRINE_BIND".into(), "127.0.0.1".into()),
        ("CITRINE_PORT".into(), "0".into()),
        (
            "CITRINE_DATA_DIR".into(),
            dir.join("data").display().to_string(),
        ),
        ("CITRINE_WEB".into(), "off".into()),
        ("CITRINE_RELAY".into(), "on".into()),
        ("CITRINE_RELAY_PASSWORD".into(), PASSWORD.into()),
        ("CITRINE_ADMIN_PASSWORD".into(), ADMIN.into()),
    ]);
    start(Config::from_vars(|k| map.get(k).cloned()).unwrap())
        .await
        .unwrap()
}

struct Client {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl Client {
    async fn connect(addr: SocketAddr) -> Self {
        let (ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/relay"))
            .await
            .unwrap();
        Self { ws }
    }

    async fn send(&mut self, msg: ClientMsg) {
        self.ws
            .send(Message::Binary(msg.encode().unwrap().into()))
            .await
            .unwrap();
    }

    async fn recv(&mut self) -> ServerMsg {
        loop {
            let frame = tokio::time::timeout(Duration::from_secs(5), self.ws.next())
                .await
                .expect("timed out waiting for the server");
            match frame {
                Some(Ok(Message::Binary(bytes))) => return ServerMsg::decode(&bytes).unwrap(),
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => {
                    panic!("the server closed the connection")
                }
                Some(Ok(_)) => continue,
            }
        }
    }

    async fn hello(addr: SocketAddr) -> (Self, ServerInfo) {
        let mut c = Self::connect(addr).await;
        c.send(ClientMsg::Hello {
            protocol: PROTOCOL_VERSION,
            app: "test".into(),
        })
        .await;
        match c.recv().await {
            ServerMsg::Info(info) => (c, info),
            other => panic!("expected info, got {other:?}"),
        }
    }

    async fn sign_up(&mut self, id: &RelayId, name: &str, invite: Option<InviteCode>) {
        let keys = id.keys();
        self.send(ClientMsg::SignUp {
            relay_password: Some(RelayPassword(PASSWORD.into())),
            auth_key: keys.auth_key().clone(),
            public_key: keys.public_key(),
            display_name: name.into(),
            invite,
        })
        .await;
    }

    async fn sign_in(&mut self, id: &RelayId) {
        self.send(ClientMsg::SignIn {
            relay_password: Some(RelayPassword(PASSWORD.into())),
            auth_key: id.keys().auth_key().clone(),
        })
        .await;
    }
}

async fn join(addr: SocketAddr, id: &RelayId, name: &str, invite: Option<InviteCode>) -> ServerMsg {
    let (mut c, _) = Client::hello(addr).await;
    c.sign_up(id, name, invite).await;
    c.recv().await
}

struct Admin {
    http: reqwest::Client,
    base: String,
    cookie: String,
}

impl Admin {
    async fn login(addr: SocketAddr) -> Self {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let base = format!("http://{addr}");
        let r = http
            .post(format!("{base}/admin/login"))
            .form(&[("password", ADMIN)])
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 303);
        let cookie = r.headers()["set-cookie"].to_str().unwrap();
        let cookie = cookie.split(';').next().unwrap().to_string();
        Self { http, base, cookie }
    }

    async fn post(&self, path: &str, form: &[(&str, &str)]) -> String {
        let r = self
            .http
            .post(format!("{}{path}", self.base))
            .header("cookie", &self.cookie)
            .form(form)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 303, "{path}");
        r.headers()["location"].to_str().unwrap().to_string()
    }

    async fn get(&self, path: &str) -> String {
        let r = self
            .http
            .get(format!("{}{path}", self.base))
            .header("cookie", &self.cookie)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200, "{path}");
        r.text().await.unwrap()
    }

    async fn require_invites(&self, on: bool) {
        let mut form = vec![
            ("name", "Citrine"),
            ("sign_ups", "on"),
            ("quota_mb", "100"),
            ("max_message_kb", "64"),
            ("max_file_mb", "25"),
        ];
        if on {
            form.push(("invites", "on"));
        }
        assert_eq!(
            self.post("/admin/settings", &form).await,
            "/admin/settings?done=saved"
        );
    }

    async fn make_invite(&self, note: &str) -> (i64, InviteCode) {
        let to = self.post("/admin/invites", &[("note", note)]).await;
        let id: i64 = to.split("new=").nth(1).unwrap().parse().unwrap();
        let page = self.get(&to).await;
        let row = page
            .split("<div class=\"invite-row")
            .find(|r| r.starts_with(" fresh"))
            .expect("the new code is highlighted");
        let code = row
            .split("data-copy=\"")
            .nth(1)
            .and_then(|r| r.split('"').next())
            .unwrap();
        (id, code.parse().unwrap())
    }
}

fn welcomed(msg: &ServerMsg) -> &citrine_gb_net::Account {
    match msg {
        ServerMsg::Welcome(account) => account,
        other => panic!("expected welcome, got {other:?}"),
    }
}

#[tokio::test]
async fn with_invites_off_nothing_changes() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path()).await;
    let (_c, info) = Client::hello(s.addr).await;
    assert!(info.sign_ups_open);
    assert!(!info.invites_required);
    let msg = join(s.addr, &RelayId::random(), "Ana", None).await;
    welcomed(&msg);
    let admin = Admin::login(s.addr).await;
    let (_, code) = admin.make_invite("unused while off").await;
    let msg = join(s.addr, &RelayId::random(), "Ben", Some(code)).await;
    welcomed(&msg);
    let page = admin.get("/admin/invites?show=unused").await;
    assert!(page.contains(&code.to_string()));
    s.stop().await;
}

#[tokio::test]
async fn with_invites_on_a_code_makes_one_account() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path()).await;
    let old_id = RelayId::random();
    welcomed(&join(s.addr, &old_id, "Old", None).await);

    let admin = Admin::login(s.addr).await;
    admin.require_invites(true).await;
    let settings = admin.get("/admin/settings").await;
    assert!(settings.contains(
        r#"id="invites" name="invites" type="checkbox" class="switch" value="on" checked"#
    ));
    let (c, info) = Client::hello(s.addr).await;
    drop(c);
    assert!(info.invites_required);

    let mia = RelayId::random();
    assert_eq!(
        join(s.addr, &mia, "Mia", None).await,
        ServerMsg::Error(ErrorCode::InviteNeeded)
    );
    let (_, code) = admin.make_invite("Mia from work").await;
    let wrong = InviteCode::new(code.value() ^ 1).unwrap();
    assert_eq!(
        join(s.addr, &mia, "Mia", Some(wrong)).await,
        ServerMsg::Error(ErrorCode::InviteWrong)
    );
    let typed: InviteCode = code
        .to_string()
        .to_lowercase()
        .replace('-', "")
        .parse()
        .unwrap();
    let msg = join(s.addr, &mia, "Mia", Some(typed)).await;
    let account = welcomed(&msg);
    assert_eq!(
        join(s.addr, &RelayId::random(), "Ben", Some(code)).await,
        ServerMsg::Error(ErrorCode::InviteUsed)
    );

    let page = admin.get("/admin/invites?show=used").await;
    assert!(page.contains("Mia from work"));
    assert!(page.contains(&account.online_id.to_string()));
    assert!(page.contains(">Mia</div>"));

    for id in [&old_id, &mia] {
        let (mut c, _) = Client::hello(s.addr).await;
        c.sign_in(id).await;
        welcomed(&c.recv().await);
    }
    s.stop().await;
}

#[tokio::test]
async fn an_unused_code_can_be_revoked_and_a_used_one_cannot() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path()).await;
    let admin = Admin::login(s.addr).await;
    admin.require_invites(true).await;
    let (unused, unused_code) = admin.make_invite("Cleo").await;
    let (used, used_code) = admin.make_invite("Dan").await;
    welcomed(&join(s.addr, &RelayId::random(), "Dan", Some(used_code)).await);

    let page = admin.get("/admin/invites").await;
    assert!(page.contains(&format!("popovertarget=\"revoke-{unused}\"")));
    assert!(!page.contains(&format!("popovertarget=\"revoke-{used}\"")));
    assert!(page.contains("Revoke the code for Cleo?"));
    let back = "/admin/invites?q=&show=all&page=1";
    assert_eq!(
        admin
            .post(
                &format!("/admin/invites/{unused}/revoke"),
                &[("back", back)]
            )
            .await,
        format!("{back}&done=revoked")
    );
    assert_eq!(
        admin
            .post(&format!("/admin/invites/{used}/revoke"), &[("back", back)])
            .await,
        format!("{back}&done=was-used")
    );
    assert_eq!(
        join(s.addr, &RelayId::random(), "Cleo", Some(unused_code)).await,
        ServerMsg::Error(ErrorCode::InviteWrong)
    );
    let page = admin.get("/admin/invites").await;
    assert!(!page.contains(&unused_code.to_string()));
    assert!(page.contains(&used_code.to_string()));
    s.stop().await;
}

#[tokio::test]
async fn a_note_is_needed_and_kept_short() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path()).await;
    let admin = Admin::login(s.addr).await;
    assert_eq!(
        admin.post("/admin/invites", &[("note", "   ")]).await,
        "/admin/invites?done=no-note"
    );
    let long = "x".repeat(81);
    assert_eq!(
        admin.post("/admin/invites", &[("note", &long)]).await,
        "/admin/invites?done=long-note"
    );
    let page = admin.get("/admin/invites").await;
    assert!(page.contains("No invite codes yet"));
    admin.make_invite("<b>Eve</b>").await;
    let page = admin.get("/admin/invites").await;
    assert!(page.contains("&lt;b&gt;Eve&lt;/b&gt;"));
    s.stop().await;
}

#[tokio::test]
async fn wrong_codes_are_rate_limited_per_address() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path()).await;
    let admin = Admin::login(s.addr).await;
    admin.require_invites(true).await;
    let (_, code) = admin.make_invite("Fay").await;
    let (mut c, _) = Client::hello(s.addr).await;
    let id = RelayId::random();
    let mut limited = false;
    for i in 0..12u64 {
        let guess = InviteCode::new(code.value() ^ (i + 1)).unwrap();
        c.sign_up(&id, "Guess", Some(guess)).await;
        match c.recv().await {
            ServerMsg::Error(ErrorCode::InviteWrong) => assert!(!limited),
            ServerMsg::Error(ErrorCode::TooManyTries) => limited = true,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
    assert!(limited);
    c.sign_up(&id, "Fay", Some(code)).await;
    assert_eq!(c.recv().await, ServerMsg::Error(ErrorCode::TooManyTries));
    s.stop().await;
}

#[tokio::test]
async fn two_sign_ups_racing_for_one_code_get_one_account() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path()).await;
    let admin = Admin::login(s.addr).await;
    admin.require_invites(true).await;
    for round in 0..5 {
        let (_, code) = admin.make_invite("race").await;
        let mut clients = Vec::new();
        for _ in 0..3 {
            clients.push(Client::hello(s.addr).await.0);
        }
        let tries = clients.into_iter().map(|mut c| async move {
            c.sign_up(&RelayId::random(), "Racer", Some(code)).await;
            c.recv().await
        });
        let answers = futures_util::future::join_all(tries).await;
        let made = answers
            .iter()
            .filter(|m| matches!(m, ServerMsg::Welcome(_)))
            .count();
        assert_eq!(made, 1, "round {round}: {answers:?}");
        assert!(answers.iter().all(|m| matches!(
            m,
            ServerMsg::Welcome(_) | ServerMsg::Error(ErrorCode::InviteUsed)
        )));
    }
    s.stop().await;
}

#[tokio::test]
async fn closed_sign_ups_win_over_invites() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path()).await;
    let admin = Admin::login(s.addr).await;
    let (_, code) = admin.make_invite("Gus").await;
    admin
        .post(
            "/admin/settings",
            &[
                ("name", "Citrine"),
                ("invites", "on"),
                ("quota_mb", "100"),
                ("max_message_kb", "64"),
                ("max_file_mb", "25"),
            ],
        )
        .await;
    let (_c, info) = Client::hello(s.addr).await;
    assert!(!info.sign_ups_open);
    assert!(!info.invites_required);
    assert_eq!(
        join(s.addr, &RelayId::random(), "Gus", Some(code)).await,
        ServerMsg::Error(ErrorCode::SignUpsClosed)
    );
    let page = admin.get("/admin/invites").await;
    assert!(page.contains("New accounts are off"));
    s.stop().await;
}

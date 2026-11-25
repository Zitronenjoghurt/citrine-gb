use citrine_gb_net::{
    ARCHIVE_PROTOCOL, ArchiveGame, ArchiveId, Channel, ClientHandshake, ClientMsg, ErrorCode,
    RelayId, RelayPassword, ServerMsg,
};
use citrine_gb_server::{AddError, Config, Running, start};
use futures_util::{SinkExt, StreamExt};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

const PASSWORD: &str = "lemon tree";
const ADMIN: &str = "admin pass";

fn config(dir: &Path, vars: &[(&str, &str)]) -> Config {
    let mut map: HashMap<String, String> = HashMap::from([
        ("CITRINE_BIND".into(), "127.0.0.1".into()),
        ("CITRINE_PORT".into(), "0".into()),
        ("CITRINE_STUN_PORT".into(), "0".into()),
        ("CITRINE_DISK_FLOOR_MB".into(), "0".into()),
        ("CITRINE_DISK_FLOOR_PERCENT".into(), "0".into()),
        ("CITRINE_IP_CONNECTIONS".into(), "100000".into()),
        ("CITRINE_IP_UNSIGNED".into(), "100000".into()),
        ("CITRINE_IP_CONNECTS_PER_MINUTE".into(), "100000".into()),
        ("CITRINE_SIGN_UPS_PER_IP".into(), "100000".into()),
        ("CITRINE_SIGN_UPS_PER_HOUR".into(), "100000".into()),
        ("CITRINE_MESSAGES_PER_SECOND".into(), "100000".into()),
        ("CITRINE_MESSAGE_BURST".into(), "100000".into()),
        (
            "CITRINE_DATA_DIR".into(),
            dir.join("data").display().to_string(),
        ),
        (
            "CITRINE_STATIC_DIR".into(),
            dir.join("static").display().to_string(),
        ),
        (
            "CITRINE_ARCHIVE_KEY_FILE".into(),
            dir.join("keys").join("archive.key").display().to_string(),
        ),
        ("CITRINE_RELAY".into(), "on".into()),
        ("CITRINE_RELAY_PASSWORD".into(), PASSWORD.into()),
        ("CITRINE_ADMIN_PASSWORD".into(), ADMIN.into()),
        ("CITRINE_ARCHIVE_COVERS".into(), "off".into()),
    ]);
    for (k, v) in vars {
        if v.is_empty() {
            map.remove(*k);
        } else {
            map.insert(k.to_string(), v.to_string());
        }
    }
    Config::from_vars(|k| map.get(k).cloned()).unwrap()
}

async fn server(dir: &Path, vars: &[(&str, &str)]) -> Running {
    start(config(dir, vars)).await.unwrap()
}

fn rom(len: usize, seed: u32) -> Vec<u8> {
    let mut x = seed | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

fn half_blank_rom(len: usize, seed: u32) -> Vec<u8> {
    let mut data = rom(len, seed);
    for (i, b) in data.iter_mut().enumerate() {
        if i % 4096 < 2048 {
            *b = 0xFF;
        }
    }
    data
}

struct Client {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    channel: Channel,
}

impl Client {
    async fn connect(addr: SocketAddr, protocol: u16) -> Self {
        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/relay"))
            .await
            .unwrap();
        let (handshake, first) = ClientHandshake::start();
        ws.send(Message::Binary(first.into())).await.unwrap();
        let reply = loop {
            match tokio::time::timeout(Duration::from_secs(5), ws.next()).await {
                Ok(Some(Ok(Message::Binary(bytes)))) => break bytes,
                Ok(Some(Ok(_))) => continue,
                other => panic!("no handshake reply: {other:?}"),
            }
        };
        let (_, channel) = handshake.finish(&reply).unwrap();
        let mut c = Self { ws, channel };
        c.send(ClientMsg::Hello {
            protocol,
            app: "test".into(),
        })
        .await;
        assert!(matches!(c.recv().await, ServerMsg::Info(_)));
        c
    }

    async fn send(&mut self, msg: ClientMsg) {
        self.ws
            .send(Message::Binary(
                self.channel.seal(&msg.encode().unwrap()).into(),
            ))
            .await
            .unwrap();
    }

    async fn recv(&mut self) -> ServerMsg {
        loop {
            let frame = tokio::time::timeout(Duration::from_secs(20), self.ws.next())
                .await
                .expect("timed out waiting for the server");
            match frame {
                Some(Ok(Message::Binary(bytes))) => {
                    match ServerMsg::decode(&self.channel.open(&bytes).unwrap()).unwrap() {
                        ServerMsg::Device(_) | ServerMsg::Devices { .. } => continue,
                        msg => return msg,
                    }
                }
                Some(Ok(_)) => continue,
                other => panic!("the server closed the connection: {other:?}"),
            }
        }
    }

    async fn until_pong(&mut self) -> Vec<ServerMsg> {
        self.send(ClientMsg::Ping(4242)).await;
        let mut seen = Vec::new();
        loop {
            match self.recv().await {
                ServerMsg::Pong(4242) => return seen,
                msg => seen.push(msg),
            }
        }
    }

    async fn archive(&mut self) -> Vec<ArchiveGame> {
        loop {
            if let ServerMsg::Archive(list) = self.recv().await {
                return list;
            }
        }
    }

    async fn sign_up(addr: SocketAddr, name: &str) -> (Self, RelayId) {
        let id = RelayId::random();
        let mut c = Self::connect(addr, ARCHIVE_PROTOCOL).await;
        let keys = id.keys();
        c.send(ClientMsg::SignUp {
            relay_password: Some(RelayPassword(PASSWORD.into())),
            auth_key: keys.auth_key().clone(),
            public_key: keys.public_key(),
            display_name: name.into(),
            invite: None,
            device_name: "Test".into(),
        })
        .await;
        assert!(matches!(c.recv().await, ServerMsg::Welcome(_)));
        (c, id)
    }

    async fn sign_in(addr: SocketAddr, id: &RelayId, protocol: u16) -> Self {
        let mut c = Self::connect(addr, protocol).await;
        c.send(ClientMsg::SignIn {
            relay_password: Some(RelayPassword(PASSWORD.into())),
            auth_key: id.keys().auth_key().clone(),
            device: None,
            code: None,
            device_name: "Test".into(),
        })
        .await;
        assert!(matches!(c.recv().await, ServerMsg::Welcome(_)));
        c
    }

    async fn download(&mut self, id: ArchiveId, from_part: u32) -> Result<Download, ErrorCode> {
        self.send(ClientMsg::ArchiveGet { id, from_part }).await;
        let (key, prefix, parts) = loop {
            match self.recv().await {
                ServerMsg::ArchiveStart {
                    id: got,
                    key,
                    prefix,
                    parts,
                } if got == id => break (key, prefix, parts),
                ServerMsg::ArchiveFailed { id: got, error } if got == id => return Err(error),
                _ => continue,
            }
        };
        let mut packed = Vec::new();
        let mut first = None;
        for _ in from_part..parts {
            match self.recv().await {
                ServerMsg::ArchivePart {
                    id: got,
                    index,
                    data,
                } if got == id => {
                    first.get_or_insert(index);
                    packed.extend(key.open_part(&prefix, index, &data).unwrap());
                }
                ServerMsg::ArchiveFailed { id: got, error } if got == id => return Err(error),
                other => panic!("expected a part, got {other:?}"),
            }
        }
        Ok(Download {
            packed,
            first,
            parts,
        })
    }
}

struct Download {
    packed: Vec<u8>,
    first: Option<u32>,
    parts: u32,
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

    async fn account(&self, name: &str) -> String {
        let page = self.get("/admin/users").await;
        manage_menu(&page, name)
            .split('"')
            .next()
            .unwrap()
            .to_string()
    }

    async fn grant(&self, name: &str) {
        let account = self.account(name).await;
        let to = self
            .post("/admin/archive/access", &[("account", &account)])
            .await;
        assert!(to.ends_with("done=archive-given"), "{to}");
    }

    async fn revoke(&self, name: &str) {
        let account = self.account(name).await;
        let to = self
            .post(&format!("/admin/archive/access/{account}/remove"), &[])
            .await;
        assert!(to.ends_with("done=archive-removed"), "{to}");
    }
}

fn unpack(packed: &[u8]) -> Vec<u8> {
    zstd::stream::decode_all(packed).unwrap()
}

fn files_under(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(files_under(&path));
        } else {
            out.push(path);
        }
    }
    out
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn manage_menu<'a>(page: &'a str, name: &str) -> &'a str {
    page.split(r#"<div id="manage-"#)
        .skip(1)
        .map(|part| {
            part.split(r#"<div class="user-row">"#)
                .next()
                .unwrap_or(part)
        })
        .find(|menu| menu.contains(&format!(r#"<div class="name">{name} "#)))
        .unwrap_or_else(|| panic!("no account named {name}"))
}

#[tokio::test]
async fn a_granted_account_gets_the_list_and_the_bytes_and_others_get_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let server = server(dir.path(), &[]).await;
    let data = half_blank_rom(1 << 20, 7);
    let added = server.add_to_archive("Test Game.gba", &data).await.unwrap();
    assert!(added.new);
    let (mut ana, _) = Client::sign_up(server.addr, "Ana").await;
    let (mut ben, _) = Client::sign_up(server.addr, "Ben").await;
    Admin::login(server.addr).await.grant("Ana").await;
    let list = ana.archive().await;
    assert_eq!(list.len(), 1);
    let game = &list[0];
    assert_eq!(game.id, added.id);
    assert_eq!(game.name, "Test Game");
    assert_eq!(game.size, data.len() as u64);
    assert_eq!(game.sha256, <[u8; 32]>::from(Sha256::digest(&data)));
    assert_eq!(game.crc32, crc32fast::hash(&data));
    assert!(game.download < game.size);
    let got = ana.download(game.id, 0).await.unwrap();
    assert_eq!(unpack(&got.packed), data);
    assert!(
        !ben.until_pong()
            .await
            .iter()
            .any(|m| matches!(m, ServerMsg::Archive(_)))
    );
    assert_eq!(
        ben.download(game.id, 0).await.err(),
        Some(ErrorCode::UnknownFile)
    );
}

#[tokio::test]
async fn a_download_resumes_from_the_part_it_asks_for() {
    let dir = tempfile::tempdir().unwrap();
    let server = server(dir.path(), &[]).await;
    let data = rom(1 << 20, 3);
    let added = server.add_to_archive("Big.nds", &data).await.unwrap();
    let (mut ana, _) = Client::sign_up(server.addr, "Ana").await;
    Admin::login(server.addr).await.grant("Ana").await;
    ana.archive().await;
    let whole = ana.download(added.id, 0).await.unwrap();
    assert!(whole.parts >= 4, "{} parts", whole.parts);
    let rest = ana.download(added.id, 2).await.unwrap();
    assert_eq!(rest.first, Some(2));
    let skipped = whole.packed.len() - rest.packed.len();
    assert_eq!(&whole.packed[skipped..], &rest.packed[..]);
    assert_eq!(
        ana.download(added.id, whole.parts).await.err(),
        Some(ErrorCode::UnknownFile)
    );
}

#[tokio::test]
async fn sign_in_sends_the_list_only_to_granted_accounts_on_new_apps() {
    let dir = tempfile::tempdir().unwrap();
    let server = server(dir.path(), &[]).await;
    server
        .add_to_archive("Test Game.gb", &half_blank_rom(64 << 10, 1))
        .await
        .unwrap();
    let (_ana, ana_id) = Client::sign_up(server.addr, "Ana").await;
    let (_ben, ben_id) = Client::sign_up(server.addr, "Ben").await;
    Admin::login(server.addr).await.grant("Ana").await;
    let mut ana = Client::sign_in(server.addr, &ana_id, ARCHIVE_PROTOCOL).await;
    assert_eq!(ana.archive().await.len(), 1);
    let mut old = Client::sign_in(server.addr, &ana_id, ARCHIVE_PROTOCOL - 1).await;
    assert!(
        !old.until_pong()
            .await
            .iter()
            .any(|m| matches!(m, ServerMsg::Archive(_)))
    );
    let mut ben = Client::sign_in(server.addr, &ben_id, ARCHIVE_PROTOCOL).await;
    assert!(
        !ben.until_pong()
            .await
            .iter()
            .any(|m| matches!(m, ServerMsg::Archive(_)))
    );
}

#[tokio::test]
async fn revoke_pushes_an_empty_list_and_refuses_downloads() {
    let dir = tempfile::tempdir().unwrap();
    let server = server(dir.path(), &[]).await;
    let added = server
        .add_to_archive("Test Game.gbc", &half_blank_rom(256 << 10, 2))
        .await
        .unwrap();
    let (mut ana, _) = Client::sign_up(server.addr, "Ana").await;
    let admin = Admin::login(server.addr).await;
    admin.grant("Ana").await;
    assert_eq!(ana.archive().await.len(), 1);
    admin.revoke("Ana").await;
    assert!(ana.archive().await.is_empty());
    assert_eq!(
        ana.download(added.id, 0).await.err(),
        Some(ErrorCode::UnknownFile)
    );
}

#[tokio::test]
async fn a_deleted_game_leaves_the_list_and_the_disk() {
    let dir = tempfile::tempdir().unwrap();
    let server = server(dir.path(), &[]).await;
    let added = server
        .add_to_archive("Test Game.gba", &half_blank_rom(256 << 10, 4))
        .await
        .unwrap();
    let (mut ana, _) = Client::sign_up(server.addr, "Ana").await;
    let admin = Admin::login(server.addr).await;
    admin.grant("Ana").await;
    assert_eq!(ana.archive().await.len(), 1);
    let stored = dir
        .path()
        .join("data")
        .join("archive")
        .join(added.id.to_string());
    assert!(stored.exists());
    let to = admin
        .post(&format!("/admin/archive/{}/delete", added.id), &[])
        .await;
    assert!(to.ends_with("done=game-deleted"), "{to}");
    assert!(ana.archive().await.is_empty());
    assert!(!stored.exists());
    let to = admin
        .post(&format!("/admin/archive/{}/delete", added.id), &[])
        .await;
    assert!(to.ends_with("done=game-gone"), "{to}");
}

#[tokio::test]
async fn the_same_game_is_stored_once() {
    let dir = tempfile::tempdir().unwrap();
    let server = server(dir.path(), &[]).await;
    let data = half_blank_rom(256 << 10, 5);
    let first = server.add_to_archive("Game.gb", &data).await.unwrap();
    let again = server.add_to_archive("Same Game.gb", &data).await.unwrap();
    assert!(first.new);
    assert!(!again.new);
    assert_eq!(first.id, again.id);
    let stored = files_under(&dir.path().join("data").join("archive"));
    assert_eq!(stored.len(), 1, "{stored:?}");
}

#[tokio::test]
async fn caps_refuse_big_files_too_many_games_and_a_full_archive() {
    let dir = tempfile::tempdir().unwrap();
    let server = server(
        dir.path(),
        &[
            ("CITRINE_ARCHIVE_FILE_MB", "1"),
            ("CITRINE_ARCHIVE_GAMES", "2"),
            ("CITRINE_ARCHIVE_TOTAL_MB", "1"),
        ],
    )
    .await;
    assert!(matches!(
        server
            .add_to_archive("Big.gba", &rom((1 << 20) + 1, 1))
            .await,
        Err(AddError::TooLarge(1))
    ));
    assert!(matches!(
        server.add_to_archive("Empty.gb", &[]).await,
        Err(AddError::Empty)
    ));
    server
        .add_to_archive("A.gb", &half_blank_rom(64 << 10, 1))
        .await
        .unwrap();
    assert!(matches!(
        server.add_to_archive("Noise.gba", &rom(1 << 20, 2)).await,
        Err(AddError::Full)
    ));
    server
        .add_to_archive("B.gb", &half_blank_rom(64 << 10, 2))
        .await
        .unwrap();
    assert!(matches!(
        server
            .add_to_archive("C.gb", &half_blank_rom(64 << 10, 4))
            .await,
        Err(AddError::TooMany(2))
    ));
    let left = files_under(&dir.path().join("data").join("archive"));
    assert_eq!(left.len(), 2, "{left:?}");
}

#[tokio::test]
async fn archive_downloads_have_their_own_daily_cap() {
    let dir = tempfile::tempdir().unwrap();
    let server = server(dir.path(), &[("CITRINE_ARCHIVE_DOWNLOAD_MB_PER_DAY", "1")]).await;
    let added = server
        .add_to_archive("Noise.gba", &rom(1 << 20, 6))
        .await
        .unwrap();
    let (mut ana, _) = Client::sign_up(server.addr, "Ana").await;
    Admin::login(server.addr).await.grant("Ana").await;
    ana.archive().await;
    ana.download(added.id, 0).await.unwrap();
    assert_eq!(
        ana.download(added.id, 0).await.err(),
        Some(ErrorCode::RateLimited)
    );
}

#[tokio::test]
async fn the_data_folder_holds_no_rom_bytes_names_or_hashes() {
    let dir = tempfile::tempdir().unwrap();
    let server = server(dir.path(), &[]).await;
    let mut data = half_blank_rom(512 << 10, 9);
    data[0x134..0x144].copy_from_slice(b"SECRETQUESTGAME!");
    let added = server
        .add_to_archive("Secret Quest.gbc", &data)
        .await
        .unwrap();
    let (mut ana, _) = Client::sign_up(server.addr, "Ana").await;
    Admin::login(server.addr).await.grant("Ana").await;
    ana.archive().await;
    ana.download(added.id, 0).await.unwrap();
    let sha256 = Sha256::digest(&data);
    let needles: [&[u8]; 5] = [
        b"Secret Quest",
        b"SECRETQUESTGAME!",
        &data[0x100..0x180],
        &data[301_056..301_120],
        &sha256[..16],
    ];
    let files = files_under(&dir.path().join("data"));
    assert!(
        files
            .iter()
            .any(|f| f.starts_with(dir.path().join("data").join("archive")))
    );
    for file in files {
        let bytes = std::fs::read(&file).unwrap();
        for needle in needles {
            assert!(
                !contains(&bytes, needle),
                "{} holds plain bytes",
                file.display()
            );
        }
    }
}

#[tokio::test]
async fn without_a_key_file_the_archive_is_off() {
    let dir = tempfile::tempdir().unwrap();
    let server = server(dir.path(), &[("CITRINE_ARCHIVE_KEY_FILE", "")]).await;
    assert!(matches!(
        server.add_to_archive("Game.gb", &rom(1024, 1)).await,
        Err(AddError::Off)
    ));
    Client::sign_up(server.addr, "Ana").await;
    let admin = Admin::login(server.addr).await;
    assert!(
        admin
            .get("/admin/archive")
            .await
            .contains("The Game Archive is off")
    );
    assert!(
        !admin
            .get("/admin/users")
            .await
            .contains("Game Archive access")
    );
}

#[tokio::test]
async fn the_admin_pages_show_games_and_who_has_access() {
    let dir = tempfile::tempdir().unwrap();
    let server = server(dir.path(), &[]).await;
    server
        .add_to_archive("Pokemon Platinum.nds", &half_blank_rom(256 << 10, 1))
        .await
        .unwrap();
    Client::sign_up(server.addr, "Ana").await;
    Client::sign_up(server.addr, "Ben").await;
    let admin = Admin::login(server.addr).await;
    admin.grant("Ana").await;
    let users = admin.get("/admin/users").await;
    let ana = manage_menu(&users, "Ana");
    let ben = manage_menu(&users, "Ben");
    assert!(ana.contains("Remove Game Archive access"));
    assert!(ana.contains(r#"archive-badge""#));
    assert!(ben.contains("Give Game Archive access"));
    assert!(!ben.contains(r#"archive-badge""#));
    let page = admin.get("/admin/archive").await;
    assert!(page.contains("Pokemon Platinum"));
    assert!(page.contains("Delete Pokemon Platinum?"));
    assert!(
        page.contains("Remove Ana&#39;s Game Archive access?")
            || page.contains("Remove Ana's Game Archive access?")
    );
    let found = admin.get("/admin/archive?q=platinum").await;
    assert!(found.contains("Pokemon Platinum"));
    let none = admin.get("/admin/archive?q=zelda").await;
    assert!(none.contains("No game matches"));
}

#[tokio::test]
async fn the_games_list_filters_by_console_and_sorts() {
    let dir = tempfile::tempdir().unwrap();
    let server = server(dir.path(), &[]).await;
    for (name, len, seed) in [
        ("Mango.gbc", 96 << 10, 3),
        ("Apple.nds", 64 << 10, 5),
        ("Zucchini.gb", 128 << 10, 7),
    ] {
        server.add_to_archive(name, &rom(len, seed)).await.unwrap();
    }
    let admin = Admin::login(server.addr).await;
    let order = |page: &str| {
        let mut at: Vec<(usize, &str)> = ["Apple", "Mango", "Zucchini"]
            .into_iter()
            .map(|n| (page.find(&format!(r#"title="{n}""#)).unwrap(), n))
            .collect();
        at.sort();
        at.into_iter().map(|(_, n)| n).collect::<Vec<_>>()
    };
    let by_name = admin.get("/admin/archive").await;
    assert_eq!(order(&by_name), ["Apple", "Mango", "Zucchini"]);
    let by_size = admin.get("/admin/archive?order=size").await;
    assert_eq!(order(&by_size), ["Zucchini", "Mango", "Apple"]);
    let ds = admin.get("/admin/archive?console=ds").await;
    assert!(ds.contains(r#"title="Apple""#));
    assert!(!ds.contains(r#"title="Mango""#));
    assert!(!ds.contains(r#"title="Zucchini""#));
    assert!(ds.contains("Nintendo DS"));
    let none = admin.get("/admin/archive?console=gba").await;
    assert!(none.contains("No game matches"));
}

#[tokio::test]
async fn the_access_mark_on_users_explains_itself() {
    let dir = tempfile::tempdir().unwrap();
    let server = server(dir.path(), &[]).await;
    Client::sign_up(server.addr, "Ana").await;
    Client::sign_up(server.addr, "Ben").await;
    let admin = Admin::login(server.addr).await;
    admin.grant("Ana").await;
    let users = admin.get("/admin/users").await;
    let rows: Vec<&str> = users.split(r#"<div class="user-row">"#).skip(1).collect();
    let row = |name: &str| {
        rows.iter()
            .find(|r| r.contains(&format!(r#"<div class="name">{name} "#)))
            .map(|r| r.split(r#"<div id="manage-"#).next().unwrap_or(r))
            .unwrap()
    };
    assert!(row("Ana").contains(r#"popover class="hint-pop">Has Game Archive access</div>"#));
    assert!(!row("Ben").contains("Has Game Archive access"));
}

#[tokio::test]
async fn the_archive_page_gives_access_by_name_or_online_id() {
    let dir = tempfile::tempdir().unwrap();
    let server = server(dir.path(), &[]).await;
    for name in ["Ana", "Ben", "Sam", "Sam"] {
        Client::sign_up(server.addr, name).await;
    }
    let admin = Admin::login(server.addr).await;
    for (who, done) in [
        (" ana ", "archive-given"),
        ("Sam", "archive-several"),
        ("Noor", "archive-nobody"),
        ("", "archive-nobody"),
    ] {
        let to = admin
            .post(
                "/admin/archive/access",
                &[("who", who), ("back", "/admin/archive")],
            )
            .await;
        assert!(to.ends_with(&format!("done={done}")), "{who}: {to}");
    }
    let users = admin.get("/admin/users").await;
    let ben = manage_menu(&users, "Ben")
        .split(r#"<span class="mono muted">"#)
        .nth(1)
        .unwrap()
        .split('<')
        .next()
        .unwrap()
        .to_string();
    let to = admin
        .post("/admin/archive/access", &[("who", ben.as_str())])
        .await;
    assert!(to.ends_with("done=archive-given"), "{to}");
    let users = admin.get("/admin/users").await;
    assert!(manage_menu(&users, "Ana").contains("Remove Game Archive access"));
    assert!(manage_menu(&users, "Ben").contains("Remove Game Archive access"));
    let page = admin.get("/admin/archive").await;
    assert!(page.contains(r#"class="access-add""#));
}

async fn upload(admin: &Admin, name: &str, data: Vec<u8>) -> (u16, Vec<serde_json::Value>) {
    let r = admin
        .http
        .post(format!("{}/admin/archive/upload", admin.base))
        .query(&[("name", name)])
        .header("cookie", &admin.cookie)
        .body(data)
        .send()
        .await
        .unwrap();
    let status = r.status().as_u16();
    let lines = r
        .text()
        .await
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    (status, lines)
}

fn leftovers(dir: &Path) -> Vec<std::path::PathBuf> {
    files_under(&dir.join("data").join("archive"))
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "upload" || e == "part"))
        .collect()
}

#[tokio::test]
async fn the_admin_page_adds_a_game_and_says_how_far_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let admin = Admin::login(s.addr).await;
    let page = admin.get("/admin/archive").await;
    assert!(page.contains("data-add-games"), "no Add games button");
    assert!(page.contains(r#"accept=".gb,.gbc,.cgb,.gba,.agb,.nds,.dsi""#));

    let data = half_blank_rom(3 << 20, 9);
    let (status, lines) = upload(&admin, "Alleyway.gb", data.clone()).await;
    assert_eq!(status, 200);
    let (last, steps) = lines.split_last().unwrap();
    assert_eq!(last["step"], "added", "{lines:?}");
    assert!(!steps.is_empty(), "no compressing steps: {lines:?}");
    for step in steps {
        assert_eq!(step["step"], "compressing");
        assert_eq!(step["of"], 3 << 20);
    }
    assert_eq!(steps.last().unwrap()["done"], 3 << 20);
    let id = last["id"].as_str().unwrap().to_string();

    let (_, again) = upload(&admin, "Alleyway (copy).gb", data).await;
    assert_eq!(again.last().unwrap()["step"], "there");
    assert_eq!(again.last().unwrap()["id"], id.as_str());
    assert_eq!(again.last().unwrap()["name"], last["name"]);
    assert!(leftovers(dir.path()).is_empty());

    let page = admin
        .get(&format!(
            "/admin/archive?done=upload&added=1&there=0&failed=0&added_game={id}&order=newest"
        ))
        .await;
    assert!(page.contains("Alleyway added"), "{page}");
    let page = admin
        .get(&format!(
            "/admin/archive?done=upload&added=2&there=1&failed=0&there_game={id}"
        ))
        .await;
    assert!(
        page.contains("2 games added. Alleyway was already in the Game Archive"),
        "{page}"
    );
}

#[tokio::test]
async fn uploads_refuse_what_the_archive_cannot_take() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[("CITRINE_ARCHIVE_FILE_MB", "1")]).await;
    let admin = Admin::login(s.addr).await;
    let (status, lines) = upload(&admin, "notes.txt", b"hello".to_vec()).await;
    assert_eq!(status, 422);
    assert_eq!(lines[0]["why"], "not-game");
    let (status, lines) = upload(&admin, "Big.gba", rom((1 << 20) + 1, 3)).await;
    assert_eq!(status, 413);
    assert_eq!(lines[0]["why"], "too-big");
    let (status, lines) = upload(&admin, "Empty.gb", Vec::new()).await;
    assert_eq!(status, 200);
    assert_eq!(lines.last().unwrap()["why"], "empty");

    let r = admin
        .http
        .post(format!(
            "{}/admin/archive/upload?name=Tetris.gb",
            admin.base
        ))
        .body(half_blank_rom(64 << 10, 3))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 303);
    assert!(leftovers(dir.path()).is_empty());
    assert!(!admin.get("/admin/archive").await.contains("Tetris"));

    let page = admin
        .get("/admin/archive?done=upload&added=2&there=0&failed=3&why=too-big&failed_file=Big.gba")
        .await;
    assert!(
        page.contains("Big.gba wasn't added: it's over the 1 MB limit. 2 more weren't added either. 2 games added"),
        "{page}"
    );
    assert!(page.contains(r#"class="flash error""#));

    let off = tempfile::tempdir().unwrap();
    let s = server(off.path(), &[("CITRINE_ARCHIVE_KEY_FILE", "")]).await;
    let admin = Admin::login(s.addr).await;
    let (status, lines) = upload(&admin, "Tetris.gb", half_blank_rom(64 << 10, 3)).await;
    assert_eq!(status, 409);
    assert_eq!(lines[0]["why"], "off");
}

async fn raw_upload(admin: &Admin, name: &str, len: usize) -> TcpStream {
    use tokio::io::AsyncWriteExt;
    let addr = admin.base.trim_start_matches("http://");
    let mut tcp = TcpStream::connect(addr).await.unwrap();
    let head = format!(
        "POST /admin/archive/upload?name={name} HTTP/1.1\r\nHost: {addr}\r\nCookie: {}\r\nContent-Length: {len}\r\n\r\n",
        admin.cookie
    );
    tcp.write_all(head.as_bytes()).await.unwrap();
    tcp
}

async fn no_leftovers(dir: &Path) {
    for _ in 0..100 {
        if leftovers(dir).is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("left behind: {:?}", leftovers(dir));
}

#[tokio::test]
async fn cancelling_an_upload_or_its_compressing_leaves_nothing() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let admin = Admin::login(s.addr).await;

    let data = half_blank_rom(8 << 20, 5);
    let mut tcp = raw_upload(&admin, "Kirby.gb", data.len()).await;
    tcp.write_all(&data[..1 << 20]).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(leftovers(dir.path()).len(), 1);
    drop(tcp);
    no_leftovers(dir.path()).await;

    let mut tcp = raw_upload(&admin, "Kirby.gb", data.len()).await;
    tcp.write_all(&data).await.unwrap();
    let mut seen = Vec::new();
    let mut buf = [0; 4096];
    while !String::from_utf8_lossy(&seen).contains("compressing") {
        let n = tcp.read(&mut buf).await.unwrap();
        assert!(n > 0, "{}", String::from_utf8_lossy(&seen));
        seen.extend_from_slice(&buf[..n]);
    }
    drop(tcp);
    no_leftovers(dir.path()).await;
    let page = admin.get("/admin/archive").await;
    assert!(!page.contains("Kirby"), "a cancelled game was added");
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (_, lines) = upload(&admin, "Kirby.gb", half_blank_rom(64 << 10, 5)).await;
    assert_eq!(
        lines.last().unwrap()["step"],
        "added",
        "the slot stayed taken"
    );
}

fn titled(at: usize, title: &[u8]) -> Vec<u8> {
    let mut data = half_blank_rom(64 << 10, 5);
    data[at..at + title.len()].copy_from_slice(title);
    data
}

#[tokio::test]
async fn an_upload_is_named_the_way_the_library_names_it() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let admin = Admin::login(s.addr).await;
    let (_, alley) = upload(
        &admin,
        "alley_way (World) [!].gb",
        titled(0x134, b"ALLEY WAY\0"),
    )
    .await;
    assert_eq!(alley.last().unwrap()["step"], "added", "{alley:?}");
    assert_eq!(alley.last().unwrap()["name"], "Alley Way", "{alley:?}");
    assert_eq!(alley.last().unwrap()["cover"], false, "{alley:?}");
    let (_, star) = upload(&admin, "(Demo).gba", titled(0xA0, b"STAR HOPPER\0")).await;
    assert_eq!(star.last().unwrap()["step"], "added", "{star:?}");
    assert!(
        alley
            .iter()
            .chain(&star)
            .all(|line| line["step"] != "cover"),
        "{alley:?} {star:?}"
    );
    let page = admin.get("/admin/archive").await;
    assert!(page.contains(">Alley Way</span>"), "{page}");
    assert!(page.contains(">Star Hopper</span>"), "{page}");
}

#[tokio::test]
async fn a_cover_shows_on_its_row_sealed_on_disk_and_only_to_the_admin() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let admin = Admin::login(s.addr).await;
    let added = s
        .add_to_archive("Petris.gb", &half_blank_rom(64 << 10, 2))
        .await
        .unwrap();
    let id = added.id.to_string();
    let page = admin.get("/admin/archive").await;
    assert!(page.contains(r#"class="cover none""#), "{page}");
    assert!(!page.contains(&format!("/admin/archive/{id}/cover")));

    let png = [b"\x89PNG\r\n\x1a\n".as_slice(), &[7; 64]].concat();
    s.put_archive_cover(added.id, &png).await.unwrap();
    let page = admin.get("/admin/archive").await;
    assert!(
        page.contains(&format!(r#"src="/admin/archive/{id}/cover""#)),
        "{page}"
    );
    let r = admin
        .http
        .get(format!("{}/admin/archive/{id}/cover", admin.base))
        .header("cookie", &admin.cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["content-type"], "image/png");
    assert_eq!(r.bytes().await.unwrap().as_ref(), png.as_slice());

    let stranger = admin
        .http
        .get(format!("{}/admin/archive/{id}/cover", admin.base))
        .send()
        .await
        .unwrap();
    assert_ne!(stranger.status(), 200);
    let on_disk = dir
        .path()
        .join("data")
        .join("archive")
        .join(format!("{id}.cover"));
    let sealed = std::fs::read(&on_disk).unwrap();
    assert!(!sealed.windows(4).any(|w| w == b"\x89PNG"));

    admin
        .post(
            &format!("/admin/archive/{id}/delete"),
            &[("back", "/admin/archive")],
        )
        .await;
    assert!(!on_disk.exists());
}

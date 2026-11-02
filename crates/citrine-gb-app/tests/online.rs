use citrine_gb_app::online::{
    JoinedRelay, Online, OnlineSettings, Phase, Problem, Seen, ServerAddress, Trust, game_title,
    seen,
};
use citrine_gb_net::{FriendState, InviteCode, OnlineId, RelayId};
use citrine_gb_server::{Config, Running, start};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;
use web_time::Instant;

const PASSWORD: &str = "lemon tree";
const ADMIN: &str = "lime";
const LIBBET: [u8; 20] = [
    0xbc, 0x17, 0x3e, 0x97, 0xe0, 0xf9, 0x50, 0x90, 0x51, 0xde, 0x84, 0xb3, 0x3b, 0x0c, 0xe9, 0xee,
    0x31, 0x84, 0xe0, 0x4e,
];

struct Relay {
    runtime: tokio::runtime::Runtime,
    running: Option<Running>,
    address: ServerAddress,
    _dir: tempfile::TempDir,
}

impl Relay {
    fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let vars: HashMap<&str, String> = HashMap::from([
            ("CITRINE_BIND", "127.0.0.1".into()),
            ("CITRINE_PORT", "0".into()),
            ("CITRINE_DISK_FLOOR_MB", "0".into()),
            ("CITRINE_DISK_FLOOR_PERCENT", "0".into()),
            (
                "CITRINE_DATA_DIR",
                dir.path().join("data").display().to_string(),
            ),
            ("CITRINE_WEB", "false".into()),
            ("CITRINE_RELAY", "true".into()),
            ("CITRINE_RELAY_PASSWORD", PASSWORD.into()),
            ("CITRINE_ADMIN_PASSWORD", ADMIN.into()),
        ]);
        let config = Config::from_vars(|k| vars.get(k).cloned()).unwrap();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let running = runtime.block_on(start(config)).unwrap();
        let address = ServerAddress::parse(&format!("127.0.0.1:{}", running.addr.port())).unwrap();
        Self {
            runtime,
            running: Some(running),
            address,
            _dir: dir,
        }
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        if let Some(running) = self.running.take() {
            self.runtime.block_on(running.stop());
        }
    }
}

fn wait(people: &mut [&mut Online], what: &str, done: impl Fn(&[&mut Online]) -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        for person in people.iter_mut() {
            person.update(Instant::now());
        }
        if done(people) {
            return;
        }
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn join(relay: &Relay, name: &str) -> (Online, JoinedRelay, OnlineId) {
    let mut online = Online::default();
    online.probe(
        relay.address.clone(),
        PASSWORD.into(),
        Trust::default(),
        Instant::now(),
    );
    wait(&mut [&mut online], "the server's info", |p| {
        p[0].phase() == Phase::Probed
    });
    let info = online.info().unwrap();
    assert!(info.sign_ups_open);
    assert!(info.password_required);
    online.create_account(name.into(), Instant::now());
    wait(&mut [&mut online], "the welcome", |p| p[0].is_connected());
    let joined = online.take_joined().expect("a join to remember");
    assert_eq!(joined.account.display_name, name);
    let id = joined.account.online_id;
    let mut settings = OnlineSettings::default();
    settings.remember(
        &joined.address,
        &joined.relay_id,
        &joined.password,
        &joined.account,
        &joined.trust,
    );
    (online, settings.joined_relay().unwrap(), id)
}

fn state(online: &Online, id: OnlineId) -> Option<FriendState> {
    online.friend(id).map(|f| f.state)
}

#[test]
fn two_friends_on_a_real_relay() {
    let relay = Relay::start();
    let (mut ana, ana_relay, ana_id) = join(&relay, "Ana");
    let (mut ben, ben_relay, ben_id) = join(&relay, "Ben");

    ana.add_friend(ben_id);
    wait(&mut [&mut ana, &mut ben], "the request", |p| {
        state(p[0], ben_id) == Some(FriendState::Outgoing)
            && state(p[1], ana_id) == Some(FriendState::Incoming)
    });
    assert_eq!(ben.waiting_requests(), 1);

    ben.answer(ana_id, true);
    wait(&mut [&mut ana, &mut ben], "the friendship", |p| {
        p[0].friends().iter().any(|f| f.online_id == ben_id)
            && p[1].friends().iter().any(|f| f.online_id == ana_id)
    });
    assert_eq!(ben.waiting_requests(), 0);
    assert_eq!(seen(ana.friend(ben_id).unwrap().presence), Seen::Online);

    ben.set_activity(false, Some(LIBBET));
    wait(&mut [&mut ana, &mut ben], "what Ben plays", |p| {
        p[0].playing(ben_id) == Some(&LIBBET)
    });
    assert_eq!(
        game_title(ana.playing(ben_id).unwrap()),
        Some("Libbet and the Magic Floor")
    );

    ben.set_activity(true, Some(LIBBET));
    wait(&mut [&mut ana, &mut ben], "Ben away", |p| {
        seen(p[0].friend(ben_id).unwrap().presence) == Seen::Away
    });

    ben.set_activity(false, None);
    wait(&mut [&mut ana, &mut ben], "Ben's game hidden", |p| {
        p[0].playing(ben_id).is_none()
            && seen(p[0].friend(ben_id).unwrap().presence) == Seen::Online
    });

    ben.set_activity(false, Some(LIBBET));
    wait(&mut [&mut ana, &mut ben], "Ben's game again", |p| {
        p[0].playing(ben_id).is_some()
    });
    ben.leave();
    wait(&mut [&mut ana], "Ben offline", |p| {
        matches!(
            seen(p[0].friend(ben_id).unwrap().presence),
            Seen::Offline { last_seen } if last_seen > 0
        ) && p[0].playing(ben_id).is_none()
    });

    ben.sign_in(ben_relay, Instant::now());
    wait(&mut [&mut ana, &mut ben], "Ben back", |p| {
        p[1].is_connected() && seen(p[0].friend(ben_id).unwrap().presence) == Seen::Online
    });
    assert!(
        ben.take_joined().is_none(),
        "signing in again is no new join"
    );
    wait(
        &mut [&mut ana, &mut ben],
        "Ben's game after the return",
        |p| p[0].playing(ben_id) == Some(&LIBBET),
    );

    ana.remove_friend(ben_id);
    wait(&mut [&mut ana, &mut ben], "the removal", |p| {
        p[0].friend(ben_id).is_none() && p[1].friend(ana_id).is_none()
    });

    ben.add_friend(ana_id);
    wait(&mut [&mut ana, &mut ben], "a new request", |p| {
        state(p[0], ben_id) == Some(FriendState::Incoming)
    });
    ana.block(ben_id);
    wait(&mut [&mut ana, &mut ben], "the block", |p| {
        state(p[0], ben_id) == Some(FriendState::Blocked) && p[1].friend(ana_id).is_none()
    });
    ben.add_friend(ana_id);
    wait(&mut [&mut ana, &mut ben], "Ben's unanswered request", |p| {
        state(p[1], ana_id) == Some(FriendState::Outgoing)
    });
    let quiet = Instant::now() + Duration::from_millis(300);
    while Instant::now() < quiet {
        ana.update(Instant::now());
        ben.update(Instant::now());
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(state(&ana, ben_id), Some(FriendState::Blocked));
    assert_eq!(ana.waiting_requests(), 0);

    let mut ana_phone = Online::default();
    ana_phone.probe(
        relay.address.clone(),
        PASSWORD.into(),
        Trust::default(),
        Instant::now(),
    );
    wait(&mut [&mut ana_phone], "the probe", |p| {
        p[0].phase() == Phase::Probed
    });
    ana_phone.use_relay_id(
        ana_relay.relay_id.to_string().parse::<RelayId>().unwrap(),
        None,
        Instant::now(),
    );
    wait(
        &mut [&mut ana_phone, &mut ana],
        "the pasted relay ID",
        |p| {
            p[0].account().map(|a| a.online_id) == Some(ana_id)
                && p[1].is_connected()
                && p[1].devices().0.len() == 2
                && state(p[0], ben_id) == Some(FriendState::Blocked)
        },
    );
}

#[test]
fn a_wrong_password_and_an_unknown_relay_id_say_so() {
    let relay = Relay::start();
    let mut online = Online::default();
    online.probe(
        relay.address.clone(),
        "wrong".into(),
        Trust::default(),
        Instant::now(),
    );
    wait(&mut [&mut online], "the probe", |p| {
        p[0].phase() == Phase::Probed
    });
    online.create_account("Ana".into(), Instant::now());
    wait(&mut [&mut online], "the refusal", |p| {
        p[0].phase() == Phase::Stopped(Problem::WrongPassword)
    });

    online.probe(
        relay.address.clone(),
        PASSWORD.into(),
        Trust::default(),
        Instant::now(),
    );
    wait(&mut [&mut online], "the probe", |p| {
        p[0].phase() == Phase::Probed
    });
    online.use_relay_id(RelayId::random(), None, Instant::now());
    wait(&mut [&mut online], "the refusal", |p| {
        p[0].phase() == Phase::Stopped(Problem::UnknownAccount)
    });
}

#[test]
fn a_server_that_is_not_there_is_unreachable() {
    let address = ServerAddress::parse("127.0.0.1:9").unwrap();
    let mut online = Online::default();
    online.probe(address, String::new(), Trust::default(), Instant::now());
    wait(&mut [&mut online], "the failure", |p| {
        p[0].phase() == Phase::Stopped(Problem::Unreachable)
    });
}

fn admin(relay: &Relay, method: &str, path: &str, cookie: &str, form: &str) -> String {
    let addr = relay.running.as_ref().unwrap().addr;
    let mut stream = TcpStream::connect(addr).unwrap();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nCookie: {cookie}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{form}",
        form.len()
    )
    .unwrap();
    let mut answer = String::new();
    stream.read_to_string(&mut answer).unwrap();
    answer
}

fn header<'a>(answer: &'a str, name: &str) -> &'a str {
    answer
        .lines()
        .find_map(|l| l.strip_prefix(name))
        .unwrap_or_else(|| panic!("no {name} in {answer}"))
        .trim()
}

fn invite_codes_on(relay: &Relay) -> (String, InviteCode) {
    let login = admin(
        relay,
        "POST",
        "/admin/login",
        "",
        &format!("password={ADMIN}"),
    );
    let cookie = header(&login, "set-cookie:")
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let saved = admin(
        relay,
        "POST",
        "/admin/settings",
        &cookie,
        "name=Lemon&sign_ups=on&invites=on&quota_mb=100&max_message_kb=64&max_file_mb=25",
    );
    assert_eq!(header(&saved, "location:"), "/admin/settings?done=saved");
    (cookie.clone(), make_invite(relay, &cookie))
}

fn make_invite(relay: &Relay, cookie: &str) -> InviteCode {
    let made = admin(relay, "POST", "/admin/invites", cookie, "note=Mia");
    let page = admin(relay, "GET", header(&made, "location:"), cookie, "");
    let code = page
        .split("data-copy=\"")
        .nth(1)
        .and_then(|r| r.split('"').next())
        .unwrap();
    code.parse().unwrap()
}

#[test]
fn a_new_account_with_an_invite_code() {
    let relay = Relay::start();
    let (cookie, code) = invite_codes_on(&relay);
    let mut mia = Online::default();
    mia.probe(
        relay.address.clone(),
        PASSWORD.into(),
        Trust::default(),
        Instant::now(),
    );
    wait(&mut [&mut mia], "the probe", |p| {
        p[0].phase() == Phase::Probed
    });
    assert!(mia.info().unwrap().invites_required);

    mia.create_account("Mia".into(), Instant::now());
    wait(&mut [&mut mia], "the refusal", |p| {
        p[0].phase() == Phase::Stopped(Problem::InviteNeeded)
    });
    let wrong = InviteCode::new(code.value() ^ 1).unwrap();
    mia.create_invited_account("Mia".into(), Some(wrong), Instant::now());
    wait(&mut [&mut mia], "the wrong code", |p| {
        p[0].phase() == Phase::Stopped(Problem::InviteWrong)
    });
    mia.create_invited_account("Mia".into(), Some(code), Instant::now());
    wait(&mut [&mut mia], "the welcome", |p| p[0].is_connected());
    let joined = mia.take_joined().expect("a join to remember");
    assert_eq!(joined.account.display_name, "Mia");

    let mut ben = Online::default();
    ben.probe(
        relay.address.clone(),
        PASSWORD.into(),
        Trust::default(),
        Instant::now(),
    );
    wait(&mut [&mut ben], "the probe", |p| {
        p[0].phase() == Phase::Probed
    });
    ben.create_invited_account("Ben".into(), Some(code), Instant::now());
    wait(&mut [&mut ben], "the used code", |p| {
        p[0].phase() == Phase::Stopped(Problem::InviteUsed)
    });
    assert_eq!(
        Problem::InviteUsed.text(),
        "That invite code was already used"
    );
    let fresh = make_invite(&relay, &cookie);
    ben.create_invited_account("Ben".into(), Some(fresh), Instant::now());
    wait(&mut [&mut ben], "Ben's welcome", |p| p[0].is_connected());
}

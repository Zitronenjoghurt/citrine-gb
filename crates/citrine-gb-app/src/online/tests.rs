use super::*;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

#[derive(Default)]
struct Wire {
    urls: Vec<String>,
    sent: Vec<ClientMsg>,
    incoming: VecDeque<TransportEvent>,
    arrived: Option<Instant>,
}

struct Fake(Rc<RefCell<Wire>>);

impl Transport for Fake {
    fn send(&mut self, bytes: Vec<u8>) {
        self.0
            .borrow_mut()
            .sent
            .push(ClientMsg::decode(&bytes).unwrap());
    }

    fn poll(&mut self) -> Option<TransportEvent> {
        self.0.borrow_mut().incoming.pop_front()
    }

    fn arrived(&self) -> Option<Instant> {
        self.0.borrow().arrived
    }
}

fn client() -> (Online, Rc<RefCell<Wire>>) {
    let wire = Rc::new(RefCell::new(Wire::default()));
    let shared = wire.clone();
    let online = Online::new(Box::new(move |url| {
        shared.borrow_mut().urls.push(url.to_owned());
        Box::new(Fake(shared.clone()))
    }));
    (online, wire)
}

fn push(wire: &Rc<RefCell<Wire>>, msg: ServerMsg) {
    wire.borrow_mut()
        .incoming
        .push_back(TransportEvent::Message(msg.encode().unwrap()));
}

fn open(wire: &Rc<RefCell<Wire>>) {
    wire.borrow_mut().incoming.push_back(TransportEvent::Opened);
}

fn sent(wire: &Rc<RefCell<Wire>>) -> Vec<ClientMsg> {
    std::mem::take(&mut wire.borrow_mut().sent)
}

fn info(sign_ups: bool) -> ServerInfo {
    ServerInfo {
        name: "Lemon".into(),
        version: "0.8.0".into(),
        protocol: PROTOCOL_VERSION,
        password_required: true,
        sign_ups_open: sign_ups,
        invites_required: false,
        quota_bytes: 100 << 20,
        max_message_bytes: 1 << 16,
        max_file_bytes: 1 << 24,
        game_sharing: false,
        pictures: true,
    }
}

fn account() -> Account {
    Account {
        online_id: OnlineId::new(0xABCD).unwrap(),
        display_name: "Clara".into(),
        storage_used: 0,
        quota_bytes: 0,
    }
}

fn joined_relay(id: &RelayId) -> JoinedRelay {
    JoinedRelay {
        address: ServerAddress::parse("gb.lemon.industries").unwrap(),
        relay_id: id.clone(),
        password: "pw".into(),
    }
}

fn signed_in(id: &RelayId) -> (Online, Rc<RefCell<Wire>>, Instant) {
    let (mut online, wire) = client();
    let now = Instant::now();
    online.sign_in(joined_relay(id), now);
    open(&wire);
    push(&wire, ServerMsg::Info(info(true)));
    push(&wire, ServerMsg::Welcome(account()));
    push(&wire, ServerMsg::Friends(vec![]));
    online.update(now);
    assert!(online.is_connected());
    sent(&wire);
    (online, wire, now)
}

#[test]
fn a_probe_reads_the_server_then_creates_the_account() {
    let (mut online, wire) = client();
    let now = Instant::now();
    online.probe(
        ServerAddress::parse("gb.lemon.industries").unwrap(),
        "pw".into(),
        now,
    );
    assert_eq!(online.phase(), Phase::Connecting);
    assert_eq!(wire.borrow().urls, ["wss://gb.lemon.industries/relay"]);

    open(&wire);
    push(&wire, ServerMsg::Info(info(true)));
    online.update(now);
    assert_eq!(online.phase(), Phase::Probed);
    assert!(
        matches!(&sent(&wire)[..], [ClientMsg::Hello { protocol, .. }] if *protocol == PROTOCOL_VERSION)
    );

    online.create_account("Clara".into(), now);
    let msgs = sent(&wire);
    let [
        ClientMsg::SignUp {
            relay_password,
            auth_key,
            display_name,
            ..
        },
    ] = &msgs[..]
    else {
        panic!("{msgs:?}")
    };
    assert_eq!(relay_password, &Some(RelayPassword("pw".into())));
    assert_eq!(display_name, "Clara");
    assert_eq!(
        wire.borrow().urls.len(),
        1,
        "the probe's connection is reused"
    );

    push(&wire, ServerMsg::Welcome(account()));
    online.update(now);
    let joined = online.take_joined().expect("a join to remember");
    assert_eq!(joined.relay_id.keys().auth_key(), auth_key);
    assert_eq!(joined.account.display_name, "Clara");
    assert!(online.take_joined().is_none());
    assert_eq!(
        online.keys().map(|k| k.public_key()),
        Some(joined.relay_id.keys().public_key()),
        "a new account can seal what it plays"
    );
}

#[test]
fn a_wrong_password_stops_and_asks() {
    let (mut online, wire) = client();
    let now = Instant::now();
    online.sign_in(joined_relay(&RelayId::random()), now);
    open(&wire);
    push(&wire, ServerMsg::Info(info(true)));
    push(&wire, ServerMsg::Error(ErrorCode::WrongPassword));
    online.update(now);
    assert_eq!(online.phase(), Phase::Stopped(Problem::WrongPassword));
    online.update(now + Duration::from_secs(600));
    assert_eq!(
        wire.borrow().urls.len(),
        1,
        "no retry with a wrong password"
    );

    online.change_password("right".into(), now);
    assert_eq!(wire.borrow().urls.len(), 2);
}

#[test]
fn an_unreachable_server_is_retried_with_growing_waits() {
    let (mut online, wire) = client();
    let mut now = Instant::now();
    online.sign_in(joined_relay(&RelayId::random()), now);
    let mut waits = Vec::new();
    for _ in 0..8 {
        wire.borrow_mut()
            .incoming
            .push_back(TransportEvent::Failed("refused".into()));
        online.update(now);
        let Phase::Retrying { at, problem } = online.phase() else {
            panic!("{:?}", online.phase())
        };
        assert_eq!(problem, Problem::Unreachable);
        waits.push(at.duration_since(now));
        now = at;
        online.update(now);
        assert_eq!(online.phase(), Phase::Connecting);
    }
    assert!(waits[0] < Duration::from_secs(2), "{waits:?}");
    assert!(waits[3] > waits[0] * 4, "{waits:?}");
    assert!(waits[7] <= Duration::from_secs(73), "{waits:?}");
    assert_eq!(wire.borrow().urls.len(), 9);
}

#[test]
fn a_silent_connect_counts_as_unreachable() {
    let (mut online, _wire) = client();
    let now = Instant::now();
    online.sign_in(joined_relay(&RelayId::random()), now);
    online.update(now + Duration::from_secs(5));
    assert_eq!(online.phase(), Phase::Connecting);
    online.update(now + Duration::from_secs(11));
    assert!(matches!(
        online.phase(),
        Phase::Retrying {
            problem: Problem::Unreachable,
            ..
        }
    ));
}

#[test]
fn the_kill_switch_waits_at_least_half_a_minute() {
    let (mut online, wire, now) = signed_in(&RelayId::random());
    push(&wire, ServerMsg::Notice(Notice::KillSwitch));
    online.update(now);
    let Phase::Retrying { at, problem } = online.phase() else {
        panic!("{:?}", online.phase())
    };
    assert_eq!(problem, Problem::KillSwitch);
    assert!(at.duration_since(now) >= Duration::from_secs(24));
}

#[test]
fn signed_in_elsewhere_does_not_fight_back() {
    let (mut online, wire, now) = signed_in(&RelayId::random());
    push(&wire, ServerMsg::Notice(Notice::SignedInElsewhere));
    online.update(now);
    assert_eq!(online.phase(), Phase::Stopped(Problem::SignedInElsewhere));
}

#[test]
fn an_invite_code_goes_with_the_sign_up_and_a_wrong_one_stops() {
    let (mut online, wire) = client();
    let now = Instant::now();
    online.probe(
        ServerAddress::parse("gb.lemon.industries").unwrap(),
        "pw".into(),
        now,
    );
    open(&wire);
    push(
        &wire,
        ServerMsg::Info(ServerInfo {
            invites_required: true,
            ..info(true)
        }),
    );
    online.update(now);
    sent(&wire);
    let code: InviteCode = "7kq2-m9xd-4htb".parse().unwrap();
    online.create_invited_account("Mia".into(), Some(code), now);
    let msgs = sent(&wire);
    assert!(
        matches!(&msgs[..], [ClientMsg::SignUp { invite: Some(c), .. }] if *c == code),
        "{msgs:?}"
    );
    push(&wire, ServerMsg::Error(ErrorCode::InviteWrong));
    online.update(now);
    assert_eq!(online.phase(), Phase::Stopped(Problem::InviteWrong));
    assert!(!Problem::InviteWrong.retries());
    assert_eq!(
        Problem::from_error(ErrorCode::InviteUsed),
        Problem::InviteUsed
    );
    assert_eq!(
        Problem::from_error(ErrorCode::InviteNeeded),
        Problem::InviteNeeded
    );
}

#[test]
fn a_join_that_fails_does_not_retry() {
    let (mut online, wire) = client();
    let now = Instant::now();
    online.probe(
        ServerAddress::parse("gb.lemon.industries").unwrap(),
        String::new(),
        now,
    );
    wire.borrow_mut()
        .incoming
        .push_back(TransportEvent::Failed("refused".into()));
    online.update(now);
    assert_eq!(online.phase(), Phase::Stopped(Problem::Unreachable));
}

#[test]
fn pings_keep_the_line_open_and_silence_drops_it() {
    let (mut online, wire, now) = signed_in(&RelayId::random());
    online.update(now + Duration::from_secs(30));
    assert!(
        sent(&wire)
            .iter()
            .any(|msg| matches!(msg, ClientMsg::Ping(_)))
    );
    online.update(now + Duration::from_secs(71));
    assert!(matches!(
        online.phase(),
        Phase::Retrying {
            problem: Problem::Lost,
            ..
        }
    ));
}

fn friend(id: &RelayId, online_id: u64, presence: Option<Presence>) -> Friend {
    Friend {
        online_id: OnlineId::new(online_id).unwrap(),
        display_name: format!("Friend {online_id}"),
        public_key: id.keys().public_key(),
        state: FriendState::Friend,
        presence,
    }
}

#[test]
fn the_game_goes_sealed_to_each_online_friend_once() {
    let me = RelayId::random();
    let (mut online, wire, now) = signed_in(&me);
    let ana = RelayId::random();
    let ben = RelayId::random();
    push(
        &wire,
        ServerMsg::Friends(vec![
            friend(&ana, 1, Some(Presence::Online { away: false })),
            friend(&ben, 2, Some(Presence::Offline { last_seen: 5 })),
        ]),
    );
    let game = [9; 20];
    online.set_activity(false, Some(game));
    online.update(now);
    let msgs = sent(&wire);
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    let ClientMsg::Playing { to, sealed } = &msgs[0] else {
        panic!("{msgs:?}")
    };
    assert_eq!(to.value(), 1);
    assert_eq!(
        ana.keys().open_playing(&me.keys().public_key(), sealed),
        Ok(Some(game))
    );

    online.update(now);
    assert!(sent(&wire).is_empty(), "nothing new to say");

    push(
        &wire,
        ServerMsg::Presence {
            who: OnlineId::new(2).unwrap(),
            presence: Presence::Online { away: false },
        },
    );
    online.update(now);
    let msgs = sent(&wire);
    assert!(
        matches!(&msgs[..], [ClientMsg::Playing { to, .. }] if to.value() == 2),
        "{msgs:?}"
    );

    push(
        &wire,
        ServerMsg::Presence {
            who: OnlineId::new(1).unwrap(),
            presence: Presence::Online { away: false },
        },
    );
    online.update(now);
    let msgs = sent(&wire);
    assert!(
        matches!(&msgs[..], [ClientMsg::Playing { to, .. }] if to.value() == 1),
        "a friend who signs in on another device hears it again: {msgs:?}"
    );

    online.set_activity(true, None);
    online.update(now);
    let msgs = sent(&wire);
    assert_eq!(msgs[0], ClientMsg::SetStatus { away: true });
    assert_eq!(msgs.len(), 3, "{msgs:?}");
}

#[test]
fn a_friends_game_opens_with_their_key_and_clears_when_they_leave() {
    let me = RelayId::random();
    let (mut online, wire, now) = signed_in(&me);
    let ana = RelayId::random();
    let ana_id = OnlineId::new(1).unwrap();
    push(
        &wire,
        ServerMsg::Friends(vec![friend(
            &ana,
            1,
            Some(Presence::Online { away: false }),
        )]),
    );
    let game = [7; 20];
    push(
        &wire,
        ServerMsg::Playing {
            from: ana_id,
            sealed: ana.keys().seal_playing(&me.keys().public_key(), Some(game)),
        },
    );
    online.update(now);
    assert_eq!(online.playing(ana_id), Some(&game));

    let eve = RelayId::random();
    push(
        &wire,
        ServerMsg::Playing {
            from: ana_id,
            sealed: eve
                .keys()
                .seal_playing(&me.keys().public_key(), Some([1; 20])),
        },
    );
    online.update(now);
    assert_eq!(
        online.playing(ana_id),
        Some(&game),
        "a forged game is ignored"
    );

    push(
        &wire,
        ServerMsg::Presence {
            who: ana_id,
            presence: Presence::Offline { last_seen: 9 },
        },
    );
    online.update(now);
    assert_eq!(online.playing(ana_id), None);
}

#[test]
fn friends_sort_online_then_away_then_recently_seen() {
    let me = RelayId::random();
    let (mut online, wire, now) = signed_in(&me);
    let key = RelayId::random();
    push(
        &wire,
        ServerMsg::Friends(vec![
            friend(&key, 1, Some(Presence::Offline { last_seen: 10 })),
            friend(&key, 2, Some(Presence::Online { away: true })),
            friend(&key, 3, Some(Presence::Offline { last_seen: 50 })),
            friend(&key, 4, Some(Presence::Online { away: false })),
            Friend {
                state: FriendState::Incoming,
                presence: None,
                ..friend(&key, 5, None)
            },
        ]),
    );
    online.update(now);
    let order: Vec<u64> = online
        .friends()
        .iter()
        .map(|f| f.online_id.value())
        .collect();
    assert_eq!(order, [4, 2, 3, 1]);
    assert_eq!(online.waiting_requests(), 1);
}

#[test]
fn messages_for_chat_and_sessions_reach_the_inbox_for_one_update() {
    let (mut online, wire, now) = signed_in(&RelayId::random());
    push(
        &wire,
        ServerMsg::Invite {
            session: 3,
            from: OnlineId::new(1).unwrap(),
            sealed: citrine_gb_net::Sealed {
                nonce: Default::default(),
                ciphertext: Vec::new(),
            },
        },
    );
    online.update(now);
    assert_eq!(online.inbox().len(), 1);
    online.update(now);
    assert!(online.inbox().is_empty());
}

#[test]
fn pictures_off_comes_from_the_server_info_or_a_refusal() {
    let (mut online, wire, now) = signed_in(&RelayId::random());
    assert!(!online.pictures_off());
    push(
        &wire,
        ServerMsg::ScreenStored {
            path: "/f/ab".into(),
        },
    );
    online.update(now);
    assert_eq!(online.screen(), Some(("/f/ab", 1)));
    push(&wire, ServerMsg::ScreenRefused(ErrorCode::RateLimited));
    online.update(now);
    assert!(!online.pictures_off());
    push(&wire, ServerMsg::ScreenRefused(ErrorCode::PicturesOff));
    online.update(now);
    assert!(online.pictures_off());
    assert_eq!(online.screen(), None);
    assert!(online.is_connected());

    let (mut online, wire) = client();
    online.sign_in(joined_relay(&RelayId::random()), now);
    open(&wire);
    push(
        &wire,
        ServerMsg::Info(ServerInfo {
            pictures: false,
            ..info(true)
        }),
    );
    online.update(now);
    assert!(online.pictures_off());
}

#[test]
fn a_failed_request_while_connected_keeps_the_connection() {
    let (mut online, wire, now) = signed_in(&RelayId::random());
    online.add_friend(OnlineId::new(77).unwrap());
    push(&wire, ServerMsg::Error(ErrorCode::UnknownUser));
    online.update(now);
    assert!(online.is_connected());
    assert_eq!(online.request_error(), Some(ErrorCode::UnknownUser));
}

fn new_account() -> (Online, Rc<RefCell<Wire>>, Instant, Keys) {
    let (mut online, wire) = client();
    let now = Instant::now();
    online.probe(
        ServerAddress::parse("gb.lemon.industries").unwrap(),
        "pw".into(),
        now,
    );
    open(&wire);
    push(&wire, ServerMsg::Info(info(true)));
    online.update(now);
    online.create_account("Clara".into(), now);
    push(&wire, ServerMsg::Welcome(account()));
    online.update(now);
    let joined = online.take_joined().expect("a join to remember");
    sent(&wire);
    (online, wire, now, joined.relay_id.keys())
}

#[test]
fn a_new_account_signs_in_when_the_connection_comes_back() {
    let (mut online, wire, now, keys) = new_account();
    wire.borrow_mut().incoming.push_back(TransportEvent::Closed);
    online.update(now);
    let Phase::Retrying { at, .. } = online.phase() else {
        panic!("{:?}", online.phase())
    };

    online.update(at);
    open(&wire);
    push(&wire, ServerMsg::Info(info(true)));
    online.update(at);
    let msgs = sent(&wire);
    assert!(
        matches!(&msgs[..], [ClientMsg::Hello { .. }, ClientMsg::SignIn { auth_key, .. }] if auth_key == keys.auth_key()),
        "{msgs:?}"
    );

    push(&wire, ServerMsg::Welcome(account()));
    online.update(at);
    assert_eq!(online.phase(), Phase::Connected);
    assert!(online.take_joined().is_none());

    online.retry_now(at);
    online.change_password("new".into(), at);
    open(&wire);
    push(&wire, ServerMsg::Info(info(true)));
    online.update(at);
    assert!(
        sent(&wire)
            .iter()
            .all(|msg| !matches!(msg, ClientMsg::SignUp { .. }))
    );
}

#[test]
fn a_sign_up_the_server_already_has_signs_in_instead() {
    let (mut online, wire) = client();
    let now = Instant::now();
    online.probe(
        ServerAddress::parse("gb.lemon.industries").unwrap(),
        "pw".into(),
        now,
    );
    open(&wire);
    push(&wire, ServerMsg::Info(info(true)));
    online.update(now);
    online.create_account("Clara".into(), now);
    let msgs = sent(&wire);
    let Some(ClientMsg::SignUp { auth_key, .. }) = msgs.last() else {
        panic!("{msgs:?}")
    };
    let auth_key = auth_key.clone();

    push(&wire, ServerMsg::Error(ErrorCode::AccountExists));
    online.update(now);
    let msgs = sent(&wire);
    assert!(
        matches!(&msgs[..], [ClientMsg::SignIn { auth_key: again, .. }] if *again == auth_key),
        "{msgs:?}"
    );
    push(&wire, ServerMsg::Welcome(account()));
    online.update(now);
    assert_eq!(online.phase(), Phase::Connected);
    assert!(online.take_joined().is_some());
}

#[test]
fn watching_the_round_trip_pings_every_few_seconds() {
    let (mut online, wire, now) = signed_in(&RelayId::random());
    assert_eq!(online.round_trip_ms(), None);
    online.watch_round_trip(now);
    let msgs = sent(&wire);
    let [ClientMsg::Ping(n)] = msgs[..] else {
        panic!("{msgs:?}")
    };
    push(&wire, ServerMsg::Pong(n));
    online.update(now + Duration::from_millis(42));
    assert_eq!(online.round_trip_ms(), Some(42));

    let later = now + Duration::from_millis(3500);
    online.watch_round_trip(later);
    online.update(later);
    assert!(matches!(sent(&wire)[..], [ClientMsg::Ping(_)]));

    let unwatched = later + Duration::from_secs(10);
    online.update(unwatched);
    sent(&wire);
    online.update(unwatched + Duration::from_secs(4));
    assert!(sent(&wire).is_empty(), "no fast pings once nobody looks");

    wire.borrow_mut().incoming.push_back(TransportEvent::Closed);
    online.update(unwatched + Duration::from_secs(5));
    assert_eq!(online.round_trip_ms(), None);
}

#[test]
fn the_round_trip_counts_when_the_pong_arrived_not_the_next_frame() {
    let (mut online, wire, now) = signed_in(&RelayId::random());
    online.watch_round_trip(now);
    let msgs = sent(&wire);
    let [ClientMsg::Ping(n)] = msgs[..] else {
        panic!("{msgs:?}")
    };
    push(&wire, ServerMsg::Pong(n));
    wire.borrow_mut().arrived = Some(now + Duration::from_millis(12));
    online.update(now + Duration::from_millis(250));
    assert_eq!(online.round_trip_ms(), Some(12));
}

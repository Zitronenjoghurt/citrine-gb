use citrine_gb_net::{
    Account, Channel, ClientHandshake, ClientMsg, EndReason, ErrorCode, FriendState, NONCE_BYTES,
    OLDEST_PROTOCOL_VERSION, OnlineId, PROTOCOL_VERSION, RelayId, RelayPassword, Sealed, ServerMsg,
    SessionId,
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

async fn server(dir: &Path, vars: &[(&str, &str)]) -> Running {
    let mut map: HashMap<String, String> = HashMap::from([
        ("CITRINE_BIND".into(), "127.0.0.1".into()),
        ("CITRINE_PORT".into(), "0".into()),
        ("CITRINE_DISK_FLOOR_MB".into(), "0".into()),
        ("CITRINE_DISK_FLOOR_PERCENT".into(), "0".into()),
        ("CITRINE_IP_CONNECTIONS".into(), "100000".into()),
        ("CITRINE_IP_UNSIGNED".into(), "100000".into()),
        ("CITRINE_IP_CONNECTS_PER_MINUTE".into(), "100000".into()),
        ("CITRINE_SIGN_UPS_PER_IP".into(), "100000".into()),
        ("CITRINE_SIGN_UPS_PER_HOUR".into(), "100000".into()),
        ("CITRINE_MESSAGES_PER_SECOND".into(), "100000".into()),
        ("CITRINE_MESSAGE_BURST".into(), "100000".into()),
        ("CITRINE_CHATS_PER_SECOND".into(), "100000".into()),
        ("CITRINE_CHAT_BURST".into(), "100000".into()),
        ("CITRINE_FRIEND_REQUESTS_PER_HOUR".into(), "100000".into()),
        ("CITRINE_FRIEND_REQUEST_BURST".into(), "100000".into()),
        (
            "CITRINE_DATA_DIR".into(),
            dir.join("data").display().to_string(),
        ),
        (
            "CITRINE_STATIC_DIR".into(),
            dir.join("static").display().to_string(),
        ),
        ("CITRINE_RELAY".into(), "on".into()),
        ("CITRINE_RELAY_PASSWORD".into(), PASSWORD.into()),
    ]);
    for (k, v) in vars {
        map.insert(k.to_string(), v.to_string());
    }
    start(Config::from_vars(|k| map.get(k).cloned()).unwrap())
        .await
        .unwrap()
}

struct Client {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    channel: Channel,
    account: Account,
    id: RelayId,
}

fn sealed(byte: u8) -> Sealed {
    Sealed {
        nonce: [byte; NONCE_BYTES],
        ciphertext: vec![byte; 40],
    }
}

fn noise(msg: &ServerMsg) -> bool {
    matches!(
        msg,
        ServerMsg::Presence { .. }
            | ServerMsg::Friend(_)
            | ServerMsg::Friends(_)
            | ServerMsg::FriendGone(_)
            | ServerMsg::Device(_)
            | ServerMsg::Devices { .. }
            | ServerMsg::Account(_)
    )
}

impl Client {
    async fn open(
        addr: SocketAddr,
        protocol: u16,
    ) -> (WebSocketStream<MaybeTlsStream<TcpStream>>, Channel) {
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
        let mut c = (ws, channel);
        send(
            &mut c.0,
            &mut c.1,
            ClientMsg::Hello {
                protocol,
                app: "test".into(),
            },
        )
        .await;
        c
    }

    async fn sign_up(addr: SocketAddr, name: &str, protocol: u16) -> Self {
        let id = RelayId::random();
        let (ws, channel) = Self::open(addr, protocol).await;
        let keys = id.keys();
        let mut c = Self {
            ws,
            channel,
            account: placeholder(),
            id,
        };
        c.send(ClientMsg::SignUp {
            relay_password: Some(RelayPassword(PASSWORD.into())),
            auth_key: keys.auth_key().clone(),
            public_key: keys.public_key(),
            display_name: name.into(),
            invite: None,
            device_name: "Test".into(),
        })
        .await;
        c.account = c.welcome().await;
        c
    }

    async fn sign_in_again(&self, addr: SocketAddr) -> Self {
        let (ws, channel) = Self::open(addr, PROTOCOL_VERSION).await;
        let mut c = Self {
            ws,
            channel,
            account: placeholder(),
            id: self.id.clone(),
        };
        c.send(ClientMsg::SignIn {
            relay_password: Some(RelayPassword(PASSWORD.into())),
            auth_key: c.id.keys().auth_key().clone(),
            device: None,
            code: None,
            device_name: "Test".into(),
        })
        .await;
        c.account = c.welcome().await;
        c
    }

    async fn welcome(&mut self) -> Account {
        loop {
            match self.recv().await {
                ServerMsg::Info(_) | ServerMsg::Device(_) => continue,
                ServerMsg::Welcome(account) => return account,
                other => panic!("expected welcome, got {other:?}"),
            }
        }
    }

    fn who(&self) -> OnlineId {
        self.account.online_id
    }

    async fn send(&mut self, msg: ClientMsg) {
        send(&mut self.ws, &mut self.channel, msg).await;
    }

    async fn recv(&mut self) -> ServerMsg {
        loop {
            let frame = tokio::time::timeout(Duration::from_secs(5), self.ws.next())
                .await
                .expect("timed out waiting for the server");
            match frame {
                Some(Ok(Message::Binary(bytes))) => {
                    return ServerMsg::decode(&self.channel.open(&bytes).unwrap()).unwrap();
                }
                Some(Ok(_)) => continue,
                other => panic!("the server closed the connection: {other:?}"),
            }
        }
    }

    async fn next(&mut self) -> ServerMsg {
        loop {
            let msg = self.recv().await;
            if !noise(&msg) {
                return msg;
            }
        }
    }

    async fn quiet(&mut self) {
        self.send(ClientMsg::Ping(4242)).await;
        assert_eq!(
            self.next().await,
            ServerMsg::Pong(4242),
            "expected nothing before the pong"
        );
    }

    async fn close(mut self) {
        self.ws.close(None).await.unwrap();
        while let Ok(Some(_)) = tokio::time::timeout(Duration::from_secs(5), self.ws.next()).await {
        }
    }
}

async fn send(
    ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    channel: &mut Channel,
    msg: ClientMsg,
) {
    ws.send(Message::Binary(channel.seal(&msg.encode().unwrap()).into()))
        .await
        .unwrap();
}

fn placeholder() -> Account {
    Account {
        online_id: OnlineId::random(),
        display_name: String::new(),
        storage_used: 0,
        quota_bytes: 0,
        two_factor: false,
        codes_left: 0,
    }
}

async fn befriend(a: &mut Client, b: &mut Client) {
    a.send(ClientMsg::AddFriend(b.who())).await;
    loop {
        if let ServerMsg::Friend(f) = b.recv().await
            && f.state == FriendState::Incoming
        {
            break;
        }
    }
    b.send(ClientMsg::AnswerFriend {
        from: a.who(),
        accept: true,
    })
    .await;
    for c in [&mut *a, &mut *b] {
        loop {
            if let ServerMsg::Friend(f) = c.recv().await
                && f.state == FriendState::Friend
            {
                break;
            }
        }
    }
}

async fn pair(a: &mut Client, b: &mut Client) -> SessionId {
    a.send(ClientMsg::Invite {
        to: b.who(),
        sealed: sealed(1),
    })
    .await;
    let ServerMsg::Invited { session, .. } = a.next().await else {
        panic!("expected invited")
    };
    assert!(matches!(b.next().await, ServerMsg::Invite { .. }));
    b.send(ClientMsg::AnswerInvite {
        session,
        accept: true,
    })
    .await;
    assert_eq!(
        b.next().await,
        ServerMsg::SessionStarted {
            session,
            with: a.who()
        }
    );
    assert_eq!(
        a.next().await,
        ServerMsg::SessionStarted {
            session,
            with: b.who()
        }
    );
    session
}

async fn room_invite(a: &mut Client, room: SessionId, to: OnlineId) -> ServerMsg {
    a.send(ClientMsg::RoomInvite {
        room,
        to,
        sealed: sealed(3),
    })
    .await;
    a.next().await
}

async fn join(a: &mut Client, c: &mut Client, room: SessionId) -> Vec<OnlineId> {
    assert_eq!(
        room_invite(a, room, c.who()).await,
        ServerMsg::Invited {
            session: room,
            to: c.who()
        }
    );
    assert_eq!(
        c.next().await,
        ServerMsg::Invite {
            session: room,
            from: a.who(),
            sealed: sealed(3),
        }
    );
    c.send(ClientMsg::AnswerInvite {
        session: room,
        accept: true,
    })
    .await;
    let ServerMsg::RoomJoined { room: got, members } = c.next().await else {
        panic!("expected room joined")
    };
    assert_eq!(got, room);
    members.into_iter().map(|m| m.who).collect()
}

async fn joined(c: &mut Client, room: SessionId, who: OnlineId) -> citrine_gb_net::Member {
    match c.next().await {
        ServerMsg::MemberUpdate { room: got, member } if got == room && member.who == who => member,
        other => panic!("expected {who} in the room, got {other:?}"),
    }
}

struct Three {
    a: Client,
    b: Client,
    c: Client,
    room: SessionId,
}

async fn three(addr: SocketAddr) -> Three {
    let mut a = Client::sign_up(addr, "Ana", PROTOCOL_VERSION).await;
    let mut b = Client::sign_up(addr, "Ben", PROTOCOL_VERSION).await;
    let mut c = Client::sign_up(addr, "Cem", PROTOCOL_VERSION).await;
    befriend(&mut a, &mut b).await;
    befriend(&mut a, &mut c).await;
    let room = pair(&mut a, &mut b).await;
    let members = join(&mut a, &mut c, room).await;
    assert_eq!(members, vec![a.who(), b.who(), c.who()]);
    let cw = c.who();
    joined(&mut a, room, cw).await;
    joined(&mut b, room, cw).await;
    Three { a, b, c, room }
}

#[tokio::test]
async fn a_third_member_joins_a_pair_and_both_hear_it() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let Three {
        mut a,
        mut b,
        mut c,
        room,
    } = three(s.addr).await;
    a.send(ClientMsg::RoomPlay {
        room,
        to: c.who(),
        sealed: sealed(5),
    })
    .await;
    assert_eq!(
        c.next().await,
        ServerMsg::RoomPlay {
            room,
            from: a.who(),
            sealed: sealed(5)
        }
    );
    b.quiet().await;
    c.send(ClientMsg::Play {
        session: room,
        sealed: sealed(6),
    })
    .await;
    assert_eq!(c.next().await, ServerMsg::Error(ErrorCode::UnknownSession));
    s.stop().await;
}

#[tokio::test]
async fn a_member_who_leaves_is_gone_and_the_room_goes_on() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let Three {
        mut a,
        mut b,
        mut c,
        room,
    } = three(s.addr).await;
    b.send(ClientMsg::Leave(room)).await;
    let gone = ServerMsg::MemberGone {
        room,
        who: b.who(),
        reason: EndReason::Left,
    };
    assert_eq!(a.next().await, gone);
    assert_eq!(c.next().await, gone);
    a.send(ClientMsg::Leave(room)).await;
    assert_eq!(
        c.next().await,
        ServerMsg::MemberGone {
            room,
            who: a.who(),
            reason: EndReason::Left,
        }
    );
    c.send(ClientMsg::RoomPlay {
        room,
        to: a.who(),
        sealed: sealed(5),
    })
    .await;
    assert_eq!(c.next().await, ServerMsg::Error(ErrorCode::UnknownSession));
    c.send(ClientMsg::MemberUpdate {
        room,
        shows_screen: true,
        playing: true,
    })
    .await;
    c.quiet().await;
    b.quiet().await;
    s.stop().await;
}

#[tokio::test]
async fn a_dropped_member_reconnects_within_the_grace() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let Three {
        mut a,
        mut b,
        c,
        room,
    } = three(s.addr).await;
    let cw = c.who();
    let again = c.sign_in_again(s.addr).await;
    c.close().await;
    for x in [&mut a, &mut b] {
        let member = joined(x, room, cw).await;
        assert!(member.state.reconnecting);
    }
    let mut c = again;
    c.send(ClientMsg::AnswerInvite {
        session: room,
        accept: true,
    })
    .await;
    let ServerMsg::RoomJoined { members, .. } = c.next().await else {
        panic!("expected room joined")
    };
    assert_eq!(members.len(), 3);
    assert!(members.iter().all(|m| !m.state.reconnecting));
    for x in [&mut a, &mut b] {
        let member = joined(x, room, cw).await;
        assert!(!member.state.reconnecting);
    }
    a.send(ClientMsg::RoomPlay {
        room,
        to: cw,
        sealed: sealed(7),
    })
    .await;
    assert!(matches!(c.next().await, ServerMsg::RoomPlay { .. }));
    s.stop().await;
}

#[tokio::test]
async fn joining_from_a_second_device_moves_the_membership() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let Three {
        mut a,
        mut b,
        mut c,
        room,
    } = three(s.addr).await;
    let cw = c.who();
    let mut again = c.sign_in_again(s.addr).await;
    again
        .send(ClientMsg::AnswerInvite {
            session: room,
            accept: true,
        })
        .await;
    let members = loop {
        if let ServerMsg::RoomJoined { members, .. } = again.next().await {
            break members;
        }
    };
    assert_eq!(members.len(), 3);
    loop {
        if let ServerMsg::MemberGone { who, reason, .. } = c.next().await {
            assert_eq!((who, reason), (cw, EndReason::Left));
            break;
        }
    }
    for x in [&mut a, &mut b] {
        assert!(!joined(x, room, cw).await.state.reconnecting);
    }
    a.send(ClientMsg::RoomPlay {
        room,
        to: cw,
        sealed: sealed(7),
    })
    .await;
    loop {
        if let ServerMsg::RoomPlay { .. } = again.next().await {
            break;
        }
    }
    s.stop().await;
}

#[tokio::test]
async fn a_drop_past_the_grace_counts_as_a_leave() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[("CITRINE_ROOM_GRACE_SECS", "1")]).await;
    let Three {
        mut a,
        mut b,
        c,
        room,
    } = three(s.addr).await;
    let cw = c.who();
    c.close().await;
    for x in [&mut a, &mut b] {
        assert!(joined(x, room, cw).await.state.reconnecting);
    }
    for x in [&mut a, &mut b] {
        assert_eq!(
            x.next().await,
            ServerMsg::MemberGone {
                room,
                who: cw,
                reason: EndReason::Disconnected,
            }
        );
    }
    s.stop().await;
}

#[tokio::test]
async fn a_full_room_refuses_the_next_member_and_the_next_player() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(
        dir.path(),
        &[("CITRINE_ROOM_MEMBERS", "3"), ("CITRINE_ROOM_PLAYERS", "2")],
    )
    .await;
    let Three {
        mut a,
        mut b,
        mut c,
        room,
    } = three(s.addr).await;
    let mut d = Client::sign_up(s.addr, "Dana", PROTOCOL_VERSION).await;
    befriend(&mut a, &mut d).await;
    assert_eq!(
        room_invite(&mut a, room, d.who()).await,
        ServerMsg::Error(ErrorCode::RoomFull)
    );
    d.quiet().await;
    for x in [&mut a, &mut b] {
        x.send(ClientMsg::MemberUpdate {
            room,
            shows_screen: true,
            playing: true,
        })
        .await;
    }
    let (aw, bw) = (a.who(), b.who());
    for _ in 0..2 {
        let ServerMsg::MemberUpdate { member, .. } = c.next().await else {
            panic!("expected a member update")
        };
        assert!(member.state.playing && [aw, bw].contains(&member.who));
    }
    c.send(ClientMsg::MemberUpdate {
        room,
        shows_screen: true,
        playing: true,
    })
    .await;
    assert_eq!(c.next().await, ServerMsg::Error(ErrorCode::RoomFull));
    s.stop().await;
}

#[tokio::test]
async fn a_block_keeps_two_people_out_of_one_room() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let Three {
        mut a,
        mut b,
        mut c,
        room,
    } = three(s.addr).await;
    let mut d = Client::sign_up(s.addr, "Dana", PROTOCOL_VERSION).await;
    let mut e = Client::sign_up(s.addr, "Emil", PROTOCOL_VERSION).await;
    befriend(&mut a, &mut d).await;
    befriend(&mut a, &mut e).await;
    b.send(ClientMsg::Block(d.who())).await;
    b.quiet().await;
    assert_eq!(
        room_invite(&mut a, room, d.who()).await,
        ServerMsg::Error(ErrorCode::CantJoin)
    );
    d.quiet().await;
    assert!(matches!(
        room_invite(&mut a, room, e.who()).await,
        ServerMsg::Invited { .. }
    ));
    assert!(matches!(e.next().await, ServerMsg::Invite { .. }));
    e.send(ClientMsg::Block(c.who())).await;
    e.quiet().await;
    e.send(ClientMsg::AnswerInvite {
        session: room,
        accept: true,
    })
    .await;
    assert_eq!(e.next().await, ServerMsg::Error(ErrorCode::CantJoin));
    a.quiet().await;
    c.quiet().await;
    s.stop().await;
}

#[tokio::test]
async fn an_old_client_keeps_a_room_of_two_on_the_old_messages() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let mut a = Client::sign_up(s.addr, "Ana", PROTOCOL_VERSION).await;
    let mut old = Client::sign_up(s.addr, "Lena", OLDEST_PROTOCOL_VERSION).await;
    let mut c = Client::sign_up(s.addr, "Cem", PROTOCOL_VERSION).await;
    befriend(&mut a, &mut old).await;
    befriend(&mut a, &mut c).await;
    let room = pair(&mut a, &mut old).await;
    let ServerMsg::MemberUpdate { member, .. } = a.next().await else {
        panic!("expected the old member's card")
    };
    assert_eq!(member.who, old.who());
    assert!(member.state.old_client);
    assert_eq!(
        room_invite(&mut a, room, c.who()).await,
        ServerMsg::Error(ErrorCode::NeedsUpdate)
    );
    c.quiet().await;
    a.send(ClientMsg::MemberUpdate {
        room,
        shows_screen: true,
        playing: true,
    })
    .await;
    a.quiet().await;
    old.quiet().await;
    old.send(ClientMsg::RoomPlay {
        room,
        to: a.who(),
        sealed: sealed(4),
    })
    .await;
    assert_eq!(
        old.next().await,
        ServerMsg::Error(ErrorCode::UnknownSession)
    );
    old.send(ClientMsg::Play {
        session: room,
        sealed: sealed(4),
    })
    .await;
    assert_eq!(
        a.next().await,
        ServerMsg::Play {
            session: room,
            sealed: sealed(4)
        }
    );
    old.send(ClientMsg::Leave(room)).await;
    assert_eq!(
        a.next().await,
        ServerMsg::MemberGone {
            room,
            who: old.who(),
            reason: EndReason::Left,
        }
    );
    old.quiet().await;
    s.stop().await;
}

#[tokio::test]
async fn an_old_client_is_not_invited_into_a_room_of_three() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let Three {
        mut a,
        mut b,
        c: _c,
        room,
    } = three(s.addr).await;
    let mut old = Client::sign_up(s.addr, "Lena", OLDEST_PROTOCOL_VERSION).await;
    befriend(&mut a, &mut old).await;
    assert_eq!(
        room_invite(&mut a, room, old.who()).await,
        ServerMsg::Error(ErrorCode::NeedsUpdate)
    );
    old.quiet().await;
    old.send(ClientMsg::AnswerInvite {
        session: room,
        accept: true,
    })
    .await;
    assert_eq!(
        old.next().await,
        ServerMsg::Error(ErrorCode::UnknownSession)
    );
    b.quiet().await;
    s.stop().await;
}

#[tokio::test]
async fn room_chat_reaches_every_member_and_is_not_stored() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let Three {
        mut a,
        mut b,
        mut c,
        room,
    } = three(s.addr).await;
    let outsider = Client::sign_up(s.addr, "Olga", PROTOCOL_VERSION).await;
    a.send(ClientMsg::RoomChat {
        room,
        copies: vec![
            (b.who(), sealed(8)),
            (c.who(), sealed(9)),
            (outsider.who(), sealed(10)),
            (a.who(), sealed(11)),
        ],
    })
    .await;
    for (x, byte) in [(&mut b, 8), (&mut c, 9)] {
        match x.next().await {
            ServerMsg::RoomChat {
                room: got,
                from,
                sealed: s,
                ..
            } => {
                assert_eq!((got, from, s), (room, a.who(), sealed(byte)));
            }
            other => panic!("expected room chat, got {other:?}"),
        }
    }
    a.quiet().await;
    let mut outsider = outsider;
    outsider.quiet().await;
    let mut later = c.sign_in_again(s.addr).await;
    later.quiet().await;
    s.stop().await;
}

use super::*;

fn mine(id: u64, delivery: Delivery) -> ChatEntry {
    ChatEntry {
        id,
        mine: true,
        at: 100,
        body: Body::Text("hi".into()),
        delivery,
        change: Change::Kept,
    }
}

#[test]
fn receipts_only_move_a_message_forward() {
    let friend = OnlineId::new(7).unwrap();
    let mut state = ChatState::default();
    for entry in [
        mine(1, Delivery::Sent),
        mine(2, Delivery::Delivered(150)),
        mine(3, Delivery::Read(160)),
        mine(4, Delivery::Sending),
        mine(5, Delivery::Failed(Failure::NotConnected)),
        mine(6, Delivery::Sent),
    ] {
        state.push(friend, entry);
    }
    let receipt = Receipt {
        delivered: vec![1, 2, 3, 4, 5, 6],
        read: vec![6],
    };
    state.take_receipt(friend, &receipt, 200);
    let got: Vec<Delivery> = state.chats[&friend]
        .entries
        .iter()
        .map(|e| e.delivery)
        .collect();
    assert_eq!(
        got,
        [
            Delivery::Delivered(200),
            Delivery::Delivered(150),
            Delivery::Read(160),
            Delivery::Sending,
            Delivery::Failed(Failure::NotConnected),
            Delivery::Read(200),
        ]
    );
}

#[test]
fn a_receipt_never_touches_received_messages() {
    let friend = OnlineId::new(7).unwrap();
    let mut state = ChatState::default();
    let mut theirs = mine(1, Delivery::Received);
    theirs.mine = false;
    state.push(friend, theirs);
    let receipt = Receipt {
        delivered: vec![1],
        read: vec![1],
    };
    state.take_receipt(friend, &receipt, 200);
    assert_eq!(state.chats[&friend].entries[0].delivery, Delivery::Received);
}

#[test]
fn old_history_still_loads_with_the_new_ticks() {
    #[derive(Serialize)]
    enum OlderDelivery {
        Sending,
        Sent,
        Failed(Failure),
        Received,
    }
    let older = postcard::to_allocvec(&[
        OlderDelivery::Sending,
        OlderDelivery::Sent,
        OlderDelivery::Failed(Failure::Gone),
        OlderDelivery::Received,
    ])
    .unwrap();
    let now: [Delivery; 4] = postcard::from_bytes(&older).unwrap();
    assert_eq!(
        now,
        [
            Delivery::Sending,
            Delivery::Sent,
            Delivery::Failed(Failure::Gone),
            Delivery::Received,
        ]
    );
}

#[test]
fn owed_receipts_go_out_in_batches() {
    let mut ids: Vec<u64> = (0..MAX_RECEIPT_IDS as u64 + 5).collect();
    assert_eq!(take_first(&mut ids).len(), MAX_RECEIPT_IDS);
    assert_eq!(
        ids,
        (MAX_RECEIPT_IDS as u64..MAX_RECEIPT_IDS as u64 + 5).collect::<Vec<_>>()
    );
}

#[test]
fn many_chats_at_once_all_arrive_under_the_relay_limits() {
    use crate::play::tests::{Pair, homebrew};
    let mut pair = Pair::new(false, 0, 0, homebrew());
    let (host, guest) = (pair.host.id(), pair.guest.id());
    for n in 0..80 {
        assert!(pair.host.online.send_text(guest, &format!("message {n}")));
    }
    pair.until("every message arrives", |p| {
        let in_flight = p.host.online.chat.sending.len();
        assert!(in_flight <= 1, "{in_flight} chats in flight to one friend");
        p.guest
            .online
            .conversation(host)
            .is_some_and(|c| c.entries.len() == 80)
    });
    let entries = &pair.host.online.conversation(guest).unwrap().entries;
    assert!(
        entries
            .iter()
            .all(|e| !matches!(e.delivery, Delivery::Failed(_)))
    );
    let texts: Vec<String> = pair
        .guest
        .online
        .conversation(host)
        .unwrap()
        .entries
        .iter()
        .filter_map(|e| match &e.body {
            Body::Text(text) => Some(text.clone()),
            _ => None,
        })
        .collect();
    let sent: Vec<String> = (0..80).map(|n| format!("message {n}")).collect();
    assert_eq!(texts, sent);
}

#[test]
fn chats_go_freely_until_a_refusal_and_paced_after() {
    let mut state = ChatState::default();
    let now = Instant::now();
    for _ in 0..200 {
        assert!(state.take_chat(now));
    }
    state.rate_limited(now);
    assert!(!state.take_chat(now + Duration::from_secs(1)));
    let now = now + RATE_LIMITED_PAUSE;
    state.chat_tokens = CHAT_BURST;
    for _ in 0..CHAT_BURST as usize {
        assert!(state.take_chat(now));
    }
    assert!(!state.take_chat(now));
    assert!(state.take_chat(now + Duration::from_millis(300)));
}

fn received(id: u64, text: &str) -> ChatEntry {
    ChatEntry {
        id,
        mine: false,
        at: 100,
        body: Body::Text(text.into()),
        delivery: Delivery::Received,
        change: Change::Kept,
    }
}

#[test]
fn a_delete_takes_the_words_and_the_unread_mark() {
    let friend = OnlineId::new(7).unwrap();
    let mut state = ChatState::default();
    state.push(friend, received(1, "secret"));
    state.push(friend, mine(2, Delivery::Sent));
    state.chats.get_mut(&friend).unwrap().unread = 1;
    state.take_delete(friend, 2);
    state.take_delete(friend, 1);
    state.take_delete(friend, 1);
    let chat = &state.chats[&friend];
    assert_eq!(chat.unread, 0);
    assert_eq!(chat.entries[0].body, Body::Text(String::new()));
    assert_eq!(chat.entries[0].change, Change::Deleted { told: true });
    assert_eq!(chat.entries[1].change, Change::Kept);
}

#[test]
fn an_edit_only_changes_text_from_the_friend() {
    let friend = OnlineId::new(7).unwrap();
    let mut state = ChatState::default();
    state.push(friend, received(1, "hi"));
    state.push(friend, mine(2, Delivery::Sent));
    let edit = |id, text: &str| Edit {
        id,
        text: text.into(),
    };
    state.take_edit(friend, &edit(1, "hello"));
    state.take_edit(friend, &edit(2, "changed"));
    state.take_edit(friend, &edit(1, "   "));
    let chat = &state.chats[&friend];
    assert_eq!(chat.entries[0].body, Body::Text("hello".into()));
    assert!(chat.entries[0].is_edited());
    assert_eq!(chat.entries[1].body, Body::Text("hi".into()));
    assert!(!chat.entries[1].is_edited());
}

#[test]
fn an_unsent_message_is_edited_quietly_and_deleted_without_a_trace() {
    let mut online = Online::default();
    let friend = OnlineId::new(7).unwrap();
    online
        .chat
        .push(friend, mine(1, Delivery::Failed(Failure::NotConnected)));
    online.chat.push(friend, mine(2, Delivery::Sent));
    assert!(online.edit_text(friend, 1, "hi there"));
    let first = &online.conversation(friend).unwrap().entries[0];
    assert_eq!(first.body, Body::Text("hi there".into()));
    assert_eq!(first.change, Change::Kept);
    online.delete_message(friend, 1);
    assert_eq!(online.conversation(friend).unwrap().entries.len(), 1);

    assert!(online.edit_text(friend, 2, "on the relay"));
    let sent = &online.conversation(friend).unwrap().entries[0];
    assert_eq!(sent.change, Change::Edited { told: false });
    online.delete_message(friend, 2);
    let sent = &online.conversation(friend).unwrap().entries[0];
    assert_eq!(sent.change, Change::Deleted { told: false });
    assert!(!online.edit_text(friend, 2, "back"));
}

fn second_device(pair: &mut crate::play::tests::Pair) -> Online {
    let joined = pair.host.online.take_joined().unwrap();
    let mut phone = Online::new(crate::online::transport::websocket());
    phone.sign_in(
        crate::online::JoinedRelay {
            address: joined.address,
            relay_id: joined.relay_id,
            password: joined.password,
            trust: crate::online::Trust {
                device: None,
                ..joined.trust
            },
        },
        Instant::now(),
    );
    phone
}

fn with_phone(
    pair: &mut crate::play::tests::Pair,
    phone: &mut Online,
    what: &str,
    mut done: impl FnMut(&crate::play::tests::Pair, &Online) -> bool,
) {
    pair.until(what, |p| {
        phone.update(Instant::now());
        done(p, phone)
    });
}

fn texts(online: &Online, friend: OnlineId) -> Vec<(bool, String)> {
    online
        .conversation(friend)
        .map(|c| {
            c.entries
                .iter()
                .filter_map(|e| match &e.body {
                    Body::Text(text) => Some((e.mine, text.clone())),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn another_device_gets_my_chats_as_mine_with_their_changes() {
    use crate::play::tests::{Pair, homebrew};
    let mut pair = Pair::new(false, 0, 0, homebrew());
    let (host, guest) = (pair.host.id(), pair.guest.id());
    assert!(pair.host.online.send_text(guest, "hi"));
    assert!(pair.host.online.send_text(guest, "bye"));
    pair.until("both arrive", |p| {
        p.guest
            .online
            .conversation(host)
            .is_some_and(|c| c.entries.len() == 2)
    });
    let ids: Vec<u64> = pair
        .host
        .online
        .conversation(guest)
        .unwrap()
        .entries
        .iter()
        .map(|e| e.id)
        .collect();
    assert!(pair.host.online.edit_text(guest, ids[0], "hello"));
    pair.host.online.delete_message(guest, ids[1]);
    pair.until("the relay knows both changes", |p| {
        let entries = &p.host.online.conversation(guest).unwrap().entries;
        entries[0].change == Change::Edited { told: true }
            && entries[1].change == Change::Deleted { told: true }
    });

    let mut phone = second_device(&mut pair);
    with_phone(
        &mut pair,
        &mut phone,
        "the phone has the edit",
        |_, phone| {
            phone
                .conversation(guest)
                .is_some_and(|c| c.entries.first().is_some_and(ChatEntry::is_edited))
        },
    );
    assert_eq!(texts(&phone, guest), [(true, "hello".into())]);
    let copy = &phone.conversation(guest).unwrap().entries[0];
    assert_eq!(copy.id, ids[0]);
    assert!(!matches!(
        copy.delivery,
        Delivery::Sending | Delivery::Failed(_)
    ));

    assert!(pair.guest.online.send_text(host, "yo"));
    with_phone(&mut pair, &mut phone, "both devices get it", |p, phone| {
        phone.unread(guest) == 1 && p.host.online.unread(guest) == 1
    });
    let theirs = pair.host.online.conversation(guest).unwrap().entries[2].id;
    pair.host.online.mark_seen(guest, &[theirs]);
    pair.host.online.mark_read(guest);
    with_phone(
        &mut pair,
        &mut phone,
        "the phone sees it was read",
        |_, phone| phone.unread(guest) == 0,
    );
    let seen = phone
        .conversation(guest)
        .unwrap()
        .entries
        .iter()
        .find(|e| e.id == theirs)
        .unwrap();
    assert_eq!(seen.delivery, Delivery::Seen);

    assert!(pair.host.online.send_text(guest, "again"));
    with_phone(
        &mut pair,
        &mut phone,
        "the phone gets it live",
        |_, phone| texts(phone, guest).last() == Some(&(true, "again".into())),
    );
    let again = pair.host.online.conversation(guest).unwrap().entries[3].id;
    pair.until("it is on the relay", |p| {
        p.host.online.conversation(guest).unwrap().entries[3].delivery != Delivery::Sending
    });
    assert!(pair.host.online.edit_text(guest, again, "again!"));
    with_phone(
        &mut pair,
        &mut phone,
        "the phone gets the edit live",
        |_, phone| texts(phone, guest).last() == Some(&(true, "again!".into())),
    );

    let (_, Some(here)) = phone.devices() else {
        panic!("the phone knows no device id");
    };
    assert!(pair.host.online.sign_out_device(here));
    let begun = Instant::now();
    with_phone(&mut pair, &mut phone, "the phone stops", |_, phone| {
        matches!(phone.phase(), crate::online::Phase::Stopped(_))
            || begun.elapsed() > Duration::from_secs(20)
    });
    assert_eq!(
        phone.phase(),
        crate::online::Phase::Stopped(crate::online::Problem::SignedOut),
        "host {:?} {:?}, phone {:?}",
        pair.host.online.request_error(),
        pair.host.online.devices(),
        phone.devices()
    );
    assert!(phone.conversation(guest).is_none());
    assert_eq!(
        pair.host.online.conversation(guest).unwrap().entries.len(),
        4
    );
}

#[test]
fn reading_the_newest_message_on_one_device_clears_the_unread_count_on_the_other() {
    use crate::play::tests::{Pair, homebrew};
    let mut pair = Pair::new(false, 0, 0, homebrew());
    let (host, guest) = (pair.host.id(), pair.guest.id());
    let mut phone = second_device(&mut pair);
    with_phone(
        &mut pair,
        &mut phone,
        "the phone is signed in",
        |_, phone| phone.friend(guest).is_some(),
    );
    assert!(pair.guest.online.send_text(host, "one"));
    assert!(pair.guest.online.send_text(host, "two"));
    with_phone(
        &mut pair,
        &mut phone,
        "both devices get both",
        |p, phone| phone.unread(guest) == 2 && p.host.online.unread(guest) == 2,
    );
    let newest = pair.host.online.conversation(guest).unwrap().entries[1].id;
    pair.host.online.mark_seen(guest, &[newest]);
    pair.host.online.mark_read(guest);
    with_phone(
        &mut pair,
        &mut phone,
        "the phone shows nothing unread",
        |_, phone| phone.unread(guest) == 0,
    );
}

#[test]
fn typing_crosses_the_relay_and_ends_on_empty_send_or_off() {
    use crate::online::typing::SHOW_FOR;
    use crate::play::tests::{Pair, homebrew};
    let mut pair = Pair::new(false, 0, 0, homebrew());
    let (host, guest) = (pair.host.id(), pair.guest.id());
    let typing = |p: &Pair| p.guest.online.is_typing(host, Instant::now());

    pair.host.online.compose(Some((guest, "")), Instant::now());
    pair.host.online.compose(Some((guest, "h")), Instant::now());
    pair.until("the friend sees the typing", typing);
    let emptied = Instant::now();
    pair.host.online.compose(Some((guest, "")), Instant::now());
    pair.until("the stop arrives", |p| !typing(p));
    assert!(emptied.elapsed() < SHOW_FOR);

    pair.host
        .online
        .compose(Some((guest, "hi")), Instant::now());
    pair.until("typing again", typing);
    assert!(pair.host.online.send_text(guest, "hi"));
    pair.host.online.compose(Some((guest, "")), Instant::now());
    pair.until("the message replaces the typing", |p| {
        !typing(p) && texts(&p.guest.online, host) == [(false, "hi".to_owned())]
    });

    pair.host.online.compose(Some((guest, "x")), Instant::now());
    pair.until("typing once more", typing);
    pair.guest
        .online
        .set_typing_indicators(false, Instant::now());
    assert!(!typing(&pair));
    pair.host.online.compose(None, Instant::now());
    pair.host.online.compose(Some((guest, "x")), Instant::now());
    pair.host
        .online
        .compose(Some((guest, "xy")), Instant::now());
    let start = Instant::now();
    pair.until("a moment passes", |_| start.elapsed() > SHOW_FOR / 5);
    assert!(!typing(&pair));
}

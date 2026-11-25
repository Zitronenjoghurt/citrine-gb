use super::*;
use citrine_gb_net::CheckNonce;

pub(super) fn asked() -> Entry {
    Entry::Asked(CheckNonce::random().commit())
}

#[test]
fn both_screens_show_the_same_code_and_allow_is_one_tap() {
    let mut host = playing("Mac");
    let mut phone = ask(&mut host, 1.0);
    assert_eq!(phone.phase(), Phase::Asking);
    let asker = host.askers().remove(0);
    assert_eq!(phone.check_code(), Some(asker.code));
    host.allow(asker.id);
    settle(&mut host, &mut phone, 1.1);
    assert_eq!(phone.phase(), Phase::Joined);
    assert_eq!(host.surfaces().len(), 1);
}

#[test]
fn a_device_in_the_middle_shows_each_screen_another_code() {
    let mut host = playing("Mac");
    let mut middle = playing("Mac");
    let (a, b) = in_app();
    middle.accept(Box::new(a));
    let mut phone = Remote::join(Box::new(b), "Pixel 8", SurfaceKind::Device, Knock::Asked);
    settle(&mut middle, &mut phone, 1.0);
    let (c, d) = in_app();
    host.accept(Box::new(c));
    let mut posing = Remote::join(Box::new(d), "Pixel 8", SurfaceKind::Device, Knock::Asked);
    settle(&mut host, &mut posing, 1.0);
    assert_eq!(phone.phase(), Phase::Asking);
    assert_eq!(posing.phase(), Phase::Asking);
    assert_eq!(middle.askers()[0].code, phone.check_code().unwrap());
    let shown = host.askers()[0].code;
    assert_eq!(posing.check_code(), Some(shown));
    assert_ne!(phone.check_code(), Some(shown));
}

#[test]
fn a_nonce_the_hello_did_not_commit_to_raises_no_prompt() {
    let mut host = playing("Mac");
    let (b, _, channel) = knock_raw(&mut host, asked(), &OPEN_LINK, 1.0);
    let mut late = Sealed::new(Box::new(b), channel.expect("an open link"));
    late.send(SurfaceMsg::Check(CheckNonce::random()).encode());
    host.step(1.1);
    assert!(host.askers().is_empty());

    let (b, _, channel) = knock_raw(&mut host, asked(), &OPEN_LINK, 1.2);
    let mut bare = Sealed::new(Box::new(b), channel.expect("an open link"));
    bare.send(SurfaceMsg::Ready.encode());
    host.step(1.3);
    assert!(host.askers().is_empty());
    assert!(host.opening.is_empty());
}

#[test]
fn asks_past_the_cap_wait_until_the_window_passes() {
    let mut host = playing("Mac");
    for _ in 0..MAX_ASK_TRIES {
        assert_eq!(ask(&mut host, 1.0).phase(), Phase::Asking);
    }
    let turned = ask(&mut host, 2.0);
    assert_eq!(turned.phase(), Phase::Ended(Some(Bye::TooManyAsks)));
    let mut own = ask_as(&mut host, "Pixel 8", None, 1.0 + ASK_TRIES_SECS);
    assert_eq!(own.phase(), Phase::Asking);
    host.allow(host.askers()[0].id);
    settle(&mut host, &mut own, 1.0 + ASK_TRIES_SECS);
    assert_eq!(own.phase(), Phase::Joined);
}

fn ask_from(host: &mut Host, last: u8, now: f64) -> Remote {
    let (near, far) = in_app();
    host.accept(Box::new(Addressed(near, IpAddr::from([192, 168, 1, last]))));
    let mut remote = Remote::join(Box::new(far), "Pixel 8", SurfaceKind::Device, Knock::Asked);
    settle(host, &mut remote, now);
    remote
}

#[test]
fn one_address_at_its_cap_leaves_another_phone_its_prompt() {
    let mut host = playing("Mac");
    for _ in 0..MAX_ASK_TRIES {
        assert_eq!(ask_from(&mut host, 66, 1.0).phase(), Phase::Asking);
    }
    let turned = ask_from(&mut host, 66, 2.0);
    assert_eq!(turned.phase(), Phase::Ended(Some(Bye::TooManyAsks)));
    let mut other = ask_from(&mut host, 7, 2.0);
    assert_eq!(other.phase(), Phase::Asking);
    let code = other.check_code().unwrap();
    let asker = host
        .askers()
        .into_iter()
        .find(|asker| asker.code == code)
        .expect("the other phone's prompt");
    host.allow(asker.id);
    settle(&mut host, &mut other, 2.1);
    assert_eq!(other.phase(), Phase::Joined);
}

#[test]
fn many_addresses_together_meet_the_cap_for_everyone() {
    let mut host = playing("Mac");
    let senders = (MAX_ASK_TRIES_ALL / MAX_ASK_TRIES) as u8;
    for last in 0..senders {
        for _ in 0..MAX_ASK_TRIES {
            assert_eq!(ask_from(&mut host, last, 1.0).phase(), Phase::Asking);
        }
    }
    let turned = ask_from(&mut host, senders, 2.0);
    assert_eq!(turned.phase(), Phase::Ended(Some(Bye::TooManyAsks)));
    let later = ask_from(&mut host, senders, 1.0 + ASK_TRIES_SECS);
    assert_eq!(later.phase(), Phase::Asking);
}

#[test]
fn a_trusted_window_needs_no_code_and_counts_no_ask() {
    let mut host = playing("Mac");
    for at in 0..MAX_ASK_TRIES + 1 {
        let (a, b) = in_app();
        host.accept_trusted(Box::new(a));
        let mut window = Remote::join(Box::new(b), "Window", SurfaceKind::Window, Knock::Asked);
        settle(&mut host, &mut window, at as f64);
        assert_eq!(window.phase(), Phase::Joined);
    }
    assert_eq!(ask(&mut host, 20.0).phase(), Phase::Asking);
}

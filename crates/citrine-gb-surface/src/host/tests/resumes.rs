use super::*;

fn asked_by_hand(host: &mut Host, now: f64) -> (Sealed, Vec<Vec<u8>>) {
    let nonce = citrine_gb_net::CheckNonce::random();
    let (b, _, channel) = knock_raw(host, Entry::Asked(nonce.commit()), &OPEN_LINK, now);
    let mut phone = Sealed::new(Box::new(b), channel.expect("an open link"));
    phone.send(SurfaceMsg::Check(nonce).encode());
    host.step(now);
    host.allow(host.askers()[0].id);
    host.step(now);
    let mut told = Vec::new();
    while let Poll::Message(bytes) = phone.poll() {
        told.push(bytes);
    }
    (phone, told)
}

fn copied_keys(told: &[Vec<u8>]) -> Vec<LinkKey> {
    let mut keys = Vec::new();
    for bytes in told {
        for w in bytes.windows(32) {
            let w: [u8; 32] = w.try_into().unwrap();
            keys.push(w);
            keys.push(resume_link(&w));
        }
    }
    keys
}

fn takes_over(host: &mut Host, id: SurfaceId, key: &LinkKey, now: f64) -> bool {
    let (b, _, channel) = knock_raw(host, Entry::Resume(id), key, now);
    let Some(channel) = channel else {
        return false;
    };
    let mut thief = Sealed::new(Box::new(b), channel);
    thief.send(SurfaceMsg::Ready.encode());
    host.step(now);
    std::iter::from_fn(|| match thief.poll() {
        Poll::Message(bytes) => Some(bytes),
        _ => None,
    })
    .any(|m| HostMsg::decode(&m).is_some_and(|m| welcomed_by("Mac")(&m)))
}

#[test]
fn a_copied_welcome_cannot_take_over_an_asked_link() {
    let mut host = playing("Mac");
    let (mut phone, told) = asked_by_hand(&mut host, 1.0);
    assert!(
        told.iter()
            .any(|m| HostMsg::decode(m).is_some_and(|m| welcomed_by("Mac")(&m)))
    );
    let id = host.surfaces()[0].id;
    for key in copied_keys(&told) {
        assert!(
            !takes_over(&mut host, id, &key, 2.0),
            "a copied token resumed the link"
        );
    }
    phone.send(SurfaceMsg::Buttons(0b10).encode());
    host.step(2.1);
    assert_eq!(host.buttons(), 0b10);
    assert_eq!(host.surfaces()[0].presence, Presence::Connected);
}

fn asked_phone(host: &mut Host, now: f64) -> Remote {
    let mut phone = ask(host, now);
    host.allow(host.askers()[0].id);
    settle(host, &mut phone, now);
    assert_eq!(phone.phase(), Phase::Joined);
    phone
}

fn drop_link(host: &mut Host, phone: &mut Remote, now: f64) {
    host.slots[0].lose(now);
    phone.cut();
    phone.step(now);
    assert_eq!(phone.phase(), Phase::Lost);
}

fn resume(host: &mut Host, phone: &mut Remote, now: f64) {
    let (a, b) = in_app();
    host.accept(Box::new(a));
    phone.resume_over(Box::new(b));
    settle(host, phone, now);
}

#[test]
fn an_asked_phone_resumes_without_a_new_ask() {
    let mut host = playing("Mac");
    let mut phone = asked_phone(&mut host, 1.0);
    let id = host.surfaces()[0].id;
    drop_link(&mut host, &mut phone, 2.0);
    resume(&mut host, &mut phone, 3.0);
    assert_eq!(phone.phase(), Phase::Joined);
    assert!(host.askers().is_empty());
    assert_eq!(host.surfaces().len(), 1);
    assert_eq!(host.surfaces()[0].id, id);
    assert_eq!(host.surfaces()[0].presence, Presence::Connected);
    phone.buttons(0b100);
    host.step(3.1);
    assert_eq!(host.buttons(), 0b100);
}

#[test]
fn a_phone_resumes_after_drops_even_one_inside_a_resume() {
    let mut host = playing("Mac");
    let mut phone = asked_phone(&mut host, 1.0);
    drop_link(&mut host, &mut phone, 2.0);
    resume(&mut host, &mut phone, 3.0);
    drop_link(&mut host, &mut phone, 4.0);

    let (a, b) = in_app();
    host.accept(Box::new(a));
    phone.resume_over(Box::new(b));
    host.step(5.0);
    phone.step(5.0);
    host.step(5.0);
    host.slots[0].lose(5.0);
    phone.cut();
    phone.step(5.0);
    assert_eq!(phone.phase(), Phase::Lost);

    resume(&mut host, &mut phone, 6.0);
    assert_eq!(phone.phase(), Phase::Joined);
    assert!(host.askers().is_empty());
    assert_eq!(host.surfaces().len(), 1);
    assert_eq!(host.surfaces()[0].presence, Presence::Connected);
    publish(&mut host, &frames(4));
    settle(&mut host, &mut phone, 6.1);
    assert_eq!(phone.picture(1).map(|p| p.rgba[0]), Some(5));
}

#[test]
fn a_resume_from_the_last_version_is_told_to_update_and_gets_no_link() {
    let mut host = playing("Mac");
    let _phone = asked_phone(&mut host, 1.0);
    let id = host.surfaces()[0].id;
    let mut hello = Hello::new("Pixel 8".into(), SurfaceKind::Device, Entry::Resume(id));
    hello.version = SURFACE_VERSION - 1;
    let (_, first) = KeyedDial::start(&hello.prologue()).unwrap();
    hello.handshake = first;
    let (a, mut b) = in_app();
    host.accept(Box::new(a));
    b.send(SurfaceMsg::Hello(hello).encode());
    host.step(2.0);
    host.step(2.1);
    assert_eq!(
        b.poll(),
        Poll::Message(HostMsg::Bye(Bye::NeedsUpdate).encode())
    );
    assert!(!matches!(b.poll(), Poll::Message(_)));
    assert!(host.opening.is_empty());
    assert_eq!(host.surfaces()[0].presence, Presence::Connected);
    assert_eq!(host.take_outdated(), vec!["Pixel 8".to_string()]);
}

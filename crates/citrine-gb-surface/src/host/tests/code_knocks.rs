use super::*;
use crate::proof::code_key;
use snow::params::{CipherChoice, DHChoice, HashChoice};
use snow::resolvers::{CryptoResolver, DefaultResolver, FallbackResolver};
use snow::types::{Cipher, Dh, Hash, Random};
use spake2::{Ed25519Group, Identity, Password, Spake2};

struct Sevens;

impl Random for Sevens {
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), snow::Error> {
        dest.fill(7);
        Ok(())
    }
}

impl rand_core::RngCore for Sevens {
    fn next_u32(&mut self) -> u32 {
        0x0707_0707
    }

    fn next_u64(&mut self) -> u64 {
        0x0707_0707_0707_0707
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        dest.fill(7);
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        dest.fill(7);
        Ok(())
    }
}

impl rand_core::CryptoRng for Sevens {}

struct SevensResolver;

impl CryptoResolver for SevensResolver {
    fn resolve_rng(&self) -> Option<Box<dyn Random>> {
        Some(Box::new(Sevens))
    }

    fn resolve_dh(&self, _: &DHChoice) -> Option<Box<dyn Dh>> {
        None
    }

    fn resolve_hash(&self, _: &HashChoice) -> Option<Box<dyn Hash>> {
        None
    }

    fn resolve_cipher(&self, _: &CipherChoice) -> Option<Box<dyn Cipher>> {
        None
    }
}

fn dialer(prologue: &[u8]) -> snow::HandshakeState {
    let resolver = FallbackResolver::new(Box::new(SevensResolver), Box::new(DefaultResolver));
    snow::Builder::with_resolver(
        "Noise_NNpsk2_25519_ChaChaPoly_BLAKE2s".parse().unwrap(),
        Box::new(resolver),
    )
    .prologue(prologue)
    .unwrap()
    .build_initiator()
    .unwrap()
}

fn first_message(state: &mut snow::HandshakeState) -> Vec<u8> {
    let mut out = vec![0; 128];
    let len = state.write_message(&[], &mut out).unwrap();
    out.truncate(len);
    out
}

fn pake_for(code: Code) -> (Spake2<Ed25519Group>, Vec<u8>) {
    Spake2::<Ed25519Group>::start_a_with_rng(
        &Password::new(code.plain()),
        &Identity::new(b"citrine screen phone"),
        &Identity::new(b"citrine screen host"),
        Sevens,
    )
}

struct Captured {
    hello: Hello,
    reply: Vec<u8>,
    pake: Vec<u8>,
}

impl Captured {
    fn opens_with(&self, code: Code) -> bool {
        let (spake, _) = pake_for(code);
        let Ok(shared) = spake.finish(&self.pake) else {
            return false;
        };
        let mut state = dialer(&self.hello.prologue());
        assert_eq!(first_message(&mut state), self.hello.handshake);
        state.set_psk(2, &code_key(&shared)).unwrap();
        let mut payload = vec![0; 128];
        state.read_message(&self.reply, &mut payload).is_ok()
    }
}

fn knock_hello(host: &mut Host, hello: &Hello, now: f64) -> (InApp, Option<HostMsg>) {
    let (a, mut b) = in_app();
    host.accept(Box::new(a));
    b.send(SurfaceMsg::Hello(hello.clone()).encode());
    host.step(now);
    let heard = match b.poll() {
        Poll::Message(bytes) => HostMsg::decode(&bytes),
        _ => None,
    };
    (b, heard)
}

fn code_hello(pake: Vec<u8>) -> Hello {
    let mut hello = Hello::new("Forger".into(), SurfaceKind::Device, Entry::Code);
    hello.pake = pake;
    hello.handshake = first_message(&mut dialer(&hello.prologue()));
    hello
}

fn knock(host: &mut Host, guess: Code, now: f64) -> (InApp, Option<HostMsg>) {
    knock_hello(host, &code_hello(pake_for(guess).1), now)
}

fn knock_once(host: &mut Host, guess: Code, now: f64) -> Option<Captured> {
    let hello = code_hello(pake_for(guess).1);
    match knock_hello(host, &hello, now).1 {
        Some(HostMsg::Opened { reply, pake }) => Some(Captured { hello, reply, pake }),
        _ => None,
    }
}

fn codes_near(code: Code) -> Vec<Code> {
    let at: u32 = code.plain().parse().unwrap();
    (at - 20..at + 20)
        .map(|n| Code::parse(&format!("{n:06}")).unwrap())
        .collect()
}

fn other_than(code: Code, n: u32) -> Code {
    let at: u32 = code.plain().parse().unwrap();
    Code::parse(&format!("{:06}", (at + n) % 1_000_000)).unwrap()
}

#[test]
fn a_captured_code_reply_checks_no_code_but_the_guess() {
    let code = Code::parse("482 913").unwrap();
    let mut host = playing("Mac");
    host.open_code_as(code, 0.0);
    let wrong = knock_once(&mut host, Code::parse("000 000").unwrap(), 1.0)
        .expect("the host answers a code knock");
    let found: Vec<Code> = codes_near(code)
        .into_iter()
        .filter(|c| wrong.opens_with(*c))
        .collect();
    assert_eq!(found, Vec::<Code>::new(), "the reply checks codes offline");

    let mut host = playing("Mac");
    host.open_code_as(code, 0.0);
    let right = knock_once(&mut host, code, 1.0).expect("the host answers a code knock");
    assert!(right.opens_with(code));
}

#[test]
fn wrong_codes_close_the_code_and_a_new_code_opens_again() {
    let mut host = playing("Mac");
    let code = host.open_code(0.0);
    for n in 1..=u32::from(MAX_CODE_TRIES) {
        let (_, heard) = knock(&mut host, other_than(code, n), 1.0);
        assert!(matches!(heard, Some(HostMsg::Opened { .. })), "try {n}");
    }
    host.step(1.1);
    assert_eq!(host.code(), None);
    assert!(host.code_spent());
    let late = pair(&mut host, Some(code), 1.2);
    assert_eq!(late.phase(), Phase::Ended(Some(Bye::WrongCode)));
    assert!(host.surfaces().is_empty());

    let code = host.open_code(2.0);
    let remote = pair(&mut host, Some(code), 2.1);
    assert_eq!(remote.phase(), Phase::Joined);
    assert!(!host.code_spent());
}

#[test]
fn the_last_try_still_pairs_with_the_right_code() {
    let mut host = playing("Mac");
    let code = host.open_code(0.0);
    for n in 1..u32::from(MAX_CODE_TRIES) {
        knock(&mut host, other_than(code, n), 1.0);
    }
    let remote = pair(&mut host, Some(code), 1.1);
    assert_eq!(remote.phase(), Phase::Joined);
    assert!(!host.code_spent());
}

#[test]
fn knocks_in_flight_count_before_they_fail() {
    let mut host = playing("Mac");
    let code = host.open_code(0.0);
    let held: Vec<InApp> = (1..=u32::from(MAX_CODE_TRIES))
        .map(|n| knock(&mut host, other_than(code, n), 1.0).0)
        .collect();
    assert_eq!(host.code().map(|(c, _)| c), Some(code));
    let (_, heard) = knock(&mut host, code, 1.1);
    assert_eq!(heard, Some(HostMsg::Bye(Bye::WrongCode)));
    drop(held);
    host.step(1.2);
    assert_eq!(host.code(), None);
    assert!(host.code_spent());
}

#[test]
fn a_garbled_pake_gets_no_answer_and_costs_no_try() {
    let mut host = playing("Mac");
    let code = host.open_code(0.0);
    for _ in 0..u32::from(MAX_CODE_TRIES) + 2 {
        let (_, heard) = knock_hello(&mut host, &code_hello(vec![1; 5]), 1.0);
        assert_eq!(heard, None);
    }
    assert_eq!(pair(&mut host, Some(code), 1.1).phase(), Phase::Joined);
}

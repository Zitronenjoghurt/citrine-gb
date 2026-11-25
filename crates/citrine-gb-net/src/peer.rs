use crate::secure::{Channel, NOISE_MAX, RelayKey, RelaySecret, SecureError, noise};
use snow::HandshakeState;

pub type PeerSecret = RelaySecret;
pub type PeerKey = RelayKey;

pub const PEER_MAGIC: [u8; 4] = *b"CGBL";
pub const MAX_SERVICE_BYTES: usize = 32;
const PAIRED: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
const ACCOUNT: &str = "Noise_XXpsk3_25519_ChaChaPoly_BLAKE2s";
const PROLOGUE: &[u8] = b"citrine-gb lan v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerHello {
    pub service: String,
    pub account: bool,
}

impl PeerHello {
    fn prologue(&self) -> Vec<u8> {
        let mut out = PROLOGUE.to_vec();
        out.push(self.account as u8);
        out.extend_from_slice(self.service.as_bytes());
        out
    }

    fn pattern(&self) -> &'static str {
        if self.account { ACCOUNT } else { PAIRED }
    }

    fn valid_service(service: &str) -> bool {
        !service.is_empty()
            && service.len() <= MAX_SERVICE_BYTES
            && service
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    }
}

pub fn peer_hello(first: &[u8]) -> Option<PeerHello> {
    split(first).map(|(hello, _)| hello)
}

fn split(first: &[u8]) -> Option<(PeerHello, &[u8])> {
    let rest = first.strip_prefix(&PEER_MAGIC)?;
    let (&account, rest) = rest.split_first()?;
    let (&len, rest) = rest.split_first()?;
    let len = len as usize;
    if account > 1 || rest.len() < len {
        return None;
    }
    let service = std::str::from_utf8(&rest[..len]).ok()?;
    if !PeerHello::valid_service(service) {
        return None;
    }
    let hello = PeerHello {
        service: service.to_owned(),
        account: account == 1,
    };
    Some((hello, &rest[len..]))
}

fn build(
    hello: &PeerHello,
    secret: &PeerSecret,
    psk: Option<&[u8; 32]>,
    initiator: bool,
) -> Result<HandshakeState, SecureError> {
    if hello.account != psk.is_some() {
        return Err(SecureError::Handshake);
    }
    let prologue = hello.prologue();
    let mut builder = noise(hello.pattern(), &prologue)
        .local_private_key(secret.as_bytes())
        .map_err(|_| SecureError::Handshake)?;
    if let Some(psk) = psk {
        builder = builder.psk(3, psk).map_err(|_| SecureError::Handshake)?;
    }
    if initiator {
        builder.build_initiator()
    } else {
        builder.build_responder()
    }
    .map_err(|_| SecureError::Handshake)
}

fn remote(state: &HandshakeState) -> Result<PeerKey, SecureError> {
    state
        .get_remote_static()
        .and_then(|k| <[u8; 32]>::try_from(k).ok())
        .map(RelayKey)
        .ok_or(SecureError::Handshake)
}

pub struct PeerDial(HandshakeState);

impl PeerDial {
    pub fn start(
        service: &str,
        secret: &PeerSecret,
        psk: Option<&[u8; 32]>,
    ) -> Result<(Self, Vec<u8>), SecureError> {
        let hello = PeerHello {
            service: service.to_owned(),
            account: psk.is_some(),
        };
        if !PeerHello::valid_service(service) {
            return Err(SecureError::Handshake);
        }
        let mut state = build(&hello, secret, psk, true)?;
        let mut out = vec![0; NOISE_MAX];
        let len = state
            .write_message(&[], &mut out)
            .map_err(|_| SecureError::Handshake)?;
        let mut first = PEER_MAGIC.to_vec();
        first.push(hello.account as u8);
        first.push(service.len() as u8);
        first.extend_from_slice(service.as_bytes());
        first.extend_from_slice(&out[..len]);
        Ok((Self(state), first))
    }

    pub fn finish(mut self, reply: &[u8]) -> Result<(Vec<u8>, PeerKey, Channel), SecureError> {
        let mut payload = vec![0; NOISE_MAX];
        self.0
            .read_message(reply, &mut payload)
            .map_err(|_| SecureError::Handshake)?;
        let key = remote(&self.0)?;
        let mut out = vec![0; NOISE_MAX];
        let len = self
            .0
            .write_message(&[], &mut out)
            .map_err(|_| SecureError::Handshake)?;
        out.truncate(len);
        Ok((out, key, Channel::new(self.0)?))
    }
}

pub struct PeerAnswer {
    state: HandshakeState,
    hello: PeerHello,
}

impl PeerAnswer {
    pub fn accept(
        first: &[u8],
        secret: &PeerSecret,
        psk: Option<&[u8; 32]>,
    ) -> Result<(Self, Vec<u8>), SecureError> {
        let (hello, message) = split(first).ok_or(SecureError::NotSecure)?;
        let mut state = build(&hello, secret, psk, false)?;
        let mut payload = vec![0; NOISE_MAX];
        state
            .read_message(message, &mut payload)
            .map_err(|_| SecureError::Handshake)?;
        let mut out = vec![0; NOISE_MAX];
        let len = state
            .write_message(&[], &mut out)
            .map_err(|_| SecureError::Handshake)?;
        out.truncate(len);
        Ok((Self { state, hello }, out))
    }

    pub fn hello(&self) -> &PeerHello {
        &self.hello
    }

    pub fn finish(mut self, last: &[u8]) -> Result<(PeerKey, Channel), SecureError> {
        let mut payload = vec![0; NOISE_MAX];
        self.state
            .read_message(last, &mut payload)
            .map_err(|_| SecureError::Handshake)?;
        let key = remote(&self.state)?;
        Ok((key, Channel::new(self.state)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Pair = (PeerKey, Channel, PeerKey, Channel);

    fn pair(
        dialer: &PeerSecret,
        dial_psk: Option<&[u8; 32]>,
        answerer: &PeerSecret,
        answer_psk: Option<&[u8; 32]>,
    ) -> Result<Pair, SecureError> {
        let (dial, first) = PeerDial::start("walker", dialer, dial_psk)?;
        let (answer, reply) = PeerAnswer::accept(&first, answerer, answer_psk)?;
        let (last, dial_saw, dial_channel) = dial.finish(&reply)?;
        let (answer_saw, answer_channel) = answer.finish(&last)?;
        Ok((dial_saw, dial_channel, answer_saw, answer_channel))
    }

    #[test]
    fn paired_devices_learn_each_others_key_and_talk() {
        let mac = PeerSecret::from_bytes([1; 32]);
        let phone = PeerSecret::from_bytes([2; 32]);
        let (mac_saw, mut mac_channel, phone_saw, mut phone_channel) =
            pair(&mac, None, &phone, None).unwrap();
        assert_eq!(mac_saw, phone.public());
        assert_eq!(phone_saw, mac.public());
        let sealed = mac_channel.seal(b"fetch");
        assert_ne!(sealed, b"fetch");
        assert_eq!(phone_channel.open(&sealed).unwrap(), b"fetch");
        assert_eq!(
            mac_channel.open(&phone_channel.seal(b"walker")).unwrap(),
            b"walker"
        );
    }

    #[test]
    fn one_account_connects_with_the_listeners_psk() {
        let keys = crate::RelayId::from_bytes([5; 32]).keys();
        let listener = 9;
        let psk = keys.walker_psk(listener);
        let same = crate::RelayId::from_bytes([5; 32])
            .keys()
            .walker_psk(listener);
        let (_, mut dial, _, mut answer) = pair(
            &PeerSecret::random(),
            Some(&psk),
            &PeerSecret::random(),
            Some(&same),
        )
        .unwrap();
        assert_eq!(answer.open(&dial.seal(b"hi")).unwrap(), b"hi");
    }

    #[test]
    fn another_account_fails_the_handshake() {
        let ours = crate::RelayId::from_bytes([5; 32]).keys().walker_psk(9);
        let theirs = crate::RelayId::from_bytes([6; 32]).keys().walker_psk(9);
        let other_device = crate::RelayId::from_bytes([5; 32]).keys().walker_psk(8);
        for psk in [theirs, other_device] {
            assert!(
                pair(
                    &PeerSecret::random(),
                    Some(&psk),
                    &PeerSecret::random(),
                    Some(&ours)
                )
                .is_err()
            );
        }
    }

    #[test]
    fn the_answerer_refuses_a_mode_it_did_not_expect() {
        let psk = [3; 32];
        let (_, first) = PeerDial::start("walker", &PeerSecret::random(), Some(&psk)).unwrap();
        assert!(PeerAnswer::accept(&first, &PeerSecret::random(), None).is_err());
        let (_, first) = PeerDial::start("walker", &PeerSecret::random(), None).unwrap();
        assert!(PeerAnswer::accept(&first, &PeerSecret::random(), Some(&psk)).is_err());
    }

    #[test]
    fn the_hello_names_the_service_and_mode() {
        let (_, first) = PeerDial::start("walker", &PeerSecret::random(), None).unwrap();
        assert_eq!(
            peer_hello(&first),
            Some(PeerHello {
                service: "walker".into(),
                account: false
            })
        );
        let (_, first) = PeerDial::start("saves", &PeerSecret::random(), Some(&[1; 32])).unwrap();
        assert_eq!(peer_hello(&first).map(|h| h.account), Some(true));
        assert!(PeerDial::start("two words", &PeerSecret::random(), None).is_err());
    }

    #[test]
    fn a_changed_service_breaks_the_handshake() {
        let dialer = PeerSecret::random();
        let answerer = PeerSecret::random();
        let (dial, mut first) = PeerDial::start("walker", &dialer, None).unwrap();
        first[6] = b'W';
        let (_, reply) = PeerAnswer::accept(&first, &answerer, None).unwrap();
        assert!(dial.finish(&reply).is_err());
    }

    #[test]
    fn other_first_frames_are_not_peer_hellos() {
        let relay = crate::ClientHandshake::start().1;
        for first in [
            &relay[..],
            &[0, 2, 3][..],
            &b"CGBL"[..],
            &b"CGBL\x02\x06walker"[..],
        ] {
            assert_eq!(peer_hello(first), None);
        }
    }
}

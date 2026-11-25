use crate::id::{ALPHABET, IdError, symbols};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use snow::params::{DHChoice, NoiseParams};
use snow::resolvers::{CryptoResolver, DefaultResolver, FallbackResolver};
use snow::types::{Cipher, Dh, Hash, Random};
use snow::{Builder, HandshakeState, StatelessTransportState};
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

pub const MAGIC: [u8; 4] = *b"CGBN";
const PATTERN: &str = "Noise_NX_25519_ChaChaPoly_BLAKE2s";
const PROLOGUE: &[u8] = b"citrine-gb relay v2";
const KEYED_PATTERN: &str = "Noise_NNpsk2_25519_ChaChaPoly_BLAKE2s";
const BINDING_CONTEXT: &[u8] = b"citrine channel binding";
pub(crate) const NOISE_MAX: usize = 65535;
const TAG: usize = 16;
const CHUNK: usize = NOISE_MAX - TAG;
const FINGERPRINT_BYTES: usize = 10;
const FINGERPRINT_CHARS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SecureError {
    #[error("the relay didn't start a secure connection")]
    NotSecure,
    #[error("the secure handshake failed")]
    Handshake,
    #[error("a message didn't decrypt")]
    Decrypt,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RelayKey(pub [u8; 32]);

impl RelayKey {
    pub fn fingerprint(&self) -> Fingerprint {
        let digest = Sha256::digest(self.0);
        let mut bytes = [0; FINGERPRINT_BYTES];
        bytes.copy_from_slice(&digest[..FINGERPRINT_BYTES]);
        Fingerprint(bytes)
    }
}

impl fmt::Debug for RelayKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RelayKey({})", self.fingerprint())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Fingerprint([u8; FINGERPRINT_BYTES]);

impl Fingerprint {
    pub fn compact(&self) -> String {
        self.to_string().replace(' ', "")
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut value = self.0.iter().fold(0u128, |v, &b| (v << 8) | b as u128);
        let mut chars = [0u8; FINGERPRINT_CHARS];
        for c in chars.iter_mut().rev() {
            *c = ALPHABET[(value & 31) as usize];
            value >>= 5;
        }
        for (i, group) in chars.chunks(4).enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            f.write_str(std::str::from_utf8(group).map_err(|_| fmt::Error)?)?;
        }
        Ok(())
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fingerprint({self})")
    }
}

impl FromStr for Fingerprint {
    type Err = IdError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let value = symbols(input, FINGERPRINT_CHARS)?
            .into_iter()
            .fold(0u128, |v, s| (v << 5) | s as u128);
        let mut bytes = [0; FINGERPRINT_BYTES];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = (value >> (8 * (FINGERPRINT_BYTES - 1 - i))) as u8;
        }
        Ok(Self(bytes))
    }
}

#[derive(Clone)]
pub struct RelaySecret([u8; 32]);

impl RelaySecret {
    pub fn random() -> Self {
        let mut bytes = [0; 32];
        getrandom::getrandom(&mut bytes).expect("no system random source");
        Self(bytes)
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn public(&self) -> RelayKey {
        let secret = crypto_box::SecretKey::from(self.0);
        RelayKey(*secret.public_key().as_bytes())
    }
}

impl fmt::Debug for RelaySecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RelaySecret(..)")
    }
}

struct SystemRandom;

impl Random for SystemRandom {
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), snow::Error> {
        getrandom::getrandom(dest).map_err(|_| snow::Error::Rng)
    }
}

struct RandomResolver;

impl CryptoResolver for RandomResolver {
    fn resolve_rng(&self) -> Option<Box<dyn Random>> {
        Some(Box::new(SystemRandom))
    }

    fn resolve_dh(&self, _: &DHChoice) -> Option<Box<dyn Dh>> {
        None
    }

    fn resolve_hash(&self, _: &snow::params::HashChoice) -> Option<Box<dyn Hash>> {
        None
    }

    fn resolve_cipher(&self, _: &snow::params::CipherChoice) -> Option<Box<dyn Cipher>> {
        None
    }
}

pub(crate) fn noise<'a>(pattern: &str, prologue: &'a [u8]) -> Builder<'a> {
    let params: NoiseParams = pattern.parse().expect("a valid noise pattern");
    let resolver = FallbackResolver::new(Box::new(RandomResolver), Box::new(DefaultResolver));
    Builder::with_resolver(params, Box::new(resolver))
        .prologue(prologue)
        .expect("a prologue")
}

fn builder<'a>() -> Builder<'a> {
    noise(PATTERN, PROLOGUE)
}

pub struct ClientHandshake(HandshakeState);

impl ClientHandshake {
    pub fn start() -> (Self, Vec<u8>) {
        let mut state = builder().build_initiator().expect("a noise initiator");
        let mut out = vec![0; NOISE_MAX];
        let len = state.write_message(&[], &mut out).expect("a first message");
        let mut first = MAGIC.to_vec();
        first.extend_from_slice(&out[..len]);
        (Self(state), first)
    }

    pub fn finish(mut self, reply: &[u8]) -> Result<(RelayKey, Channel), SecureError> {
        let mut payload = vec![0; NOISE_MAX];
        self.0
            .read_message(reply, &mut payload)
            .map_err(|_| SecureError::Handshake)?;
        let key = self
            .0
            .get_remote_static()
            .and_then(|k| <[u8; 32]>::try_from(k).ok())
            .map(RelayKey)
            .ok_or(SecureError::Handshake)?;
        let channel = Channel::new(self.0)?;
        Ok((key, channel))
    }
}

pub fn is_secure_start(first: &[u8]) -> bool {
    first.starts_with(&MAGIC)
}

pub fn accept(secret: &RelaySecret, first: &[u8]) -> Result<(Vec<u8>, Channel), SecureError> {
    let message = first.strip_prefix(&MAGIC).ok_or(SecureError::NotSecure)?;
    let mut state = builder()
        .local_private_key(secret.as_bytes())
        .and_then(|b| b.build_responder())
        .map_err(|_| SecureError::Handshake)?;
    let mut payload = vec![0; NOISE_MAX];
    state
        .read_message(message, &mut payload)
        .map_err(|_| SecureError::Handshake)?;
    let mut out = vec![0; NOISE_MAX];
    let len = state
        .write_message(&[], &mut out)
        .map_err(|_| SecureError::Handshake)?;
    out.truncate(len);
    Ok((out, Channel::new(state)?))
}

fn keyed<'a>(prologue: &'a [u8], key: &'a [u8; 32]) -> Result<Builder<'a>, SecureError> {
    noise(KEYED_PATTERN, prologue)
        .psk(2, key)
        .map_err(|_| SecureError::Handshake)
}

pub struct KeyedDial(HandshakeState);

impl KeyedDial {
    pub fn start(prologue: &[u8]) -> Result<(Self, Vec<u8>), SecureError> {
        let mut state = noise(KEYED_PATTERN, prologue)
            .build_initiator()
            .map_err(|_| SecureError::Handshake)?;
        let mut out = vec![0; NOISE_MAX];
        let len = state
            .write_message(&[], &mut out)
            .map_err(|_| SecureError::Handshake)?;
        out.truncate(len);
        Ok((Self(state), out))
    }

    pub fn finish(mut self, key: &[u8; 32], reply: &[u8]) -> Result<Channel, SecureError> {
        self.0.set_psk(2, key).map_err(|_| SecureError::Handshake)?;
        let mut payload = vec![0; NOISE_MAX];
        self.0
            .read_message(reply, &mut payload)
            .map_err(|_| SecureError::Handshake)?;
        Channel::new(self.0)
    }
}

pub fn keyed_answer(
    prologue: &[u8],
    key: &[u8; 32],
    first: &[u8],
) -> Result<(Vec<u8>, Channel), SecureError> {
    let mut state = keyed(prologue, key)?
        .build_responder()
        .map_err(|_| SecureError::Handshake)?;
    let mut payload = vec![0; NOISE_MAX];
    state
        .read_message(first, &mut payload)
        .map_err(|_| SecureError::Handshake)?;
    let mut out = vec![0; NOISE_MAX];
    let len = state
        .write_message(&[], &mut out)
        .map_err(|_| SecureError::Handshake)?;
    out.truncate(len);
    Ok((out, Channel::new(state)?))
}

pub struct Channel {
    sealer: Sealer,
    opener: Opener,
    binding: [u8; 32],
}

impl Channel {
    pub(crate) fn new(mut state: HandshakeState) -> Result<Self, SecureError> {
        let (one, two) = state.dangerously_get_raw_split();
        let binding = Sha256::new()
            .chain_update(BINDING_CONTEXT)
            .chain_update(one)
            .chain_update(two)
            .finalize()
            .into();
        let state = Arc::new(
            state
                .into_stateless_transport_mode()
                .map_err(|_| SecureError::Handshake)?,
        );
        Ok(Self {
            sealer: Sealer {
                state: state.clone(),
                nonce: 0,
            },
            opener: Opener { state, nonce: 0 },
            binding,
        })
    }

    pub fn binding(&self) -> [u8; 32] {
        self.binding
    }

    pub fn split(self) -> (Sealer, Opener) {
        (self.sealer, self.opener)
    }

    pub fn seal(&mut self, plain: &[u8]) -> Vec<u8> {
        self.sealer.seal(plain)
    }

    pub fn open(&mut self, sealed: &[u8]) -> Result<Vec<u8>, SecureError> {
        self.opener.open(sealed)
    }
}

pub struct Sealer {
    state: Arc<StatelessTransportState>,
    nonce: u64,
}

impl Sealer {
    pub fn seal(&mut self, plain: &[u8]) -> Vec<u8> {
        let chunks = plain.len().div_ceil(CHUNK).max(1);
        let mut out = vec![0; plain.len() + chunks * TAG];
        let mut at = 0;
        for i in 0..chunks {
            let piece = &plain[(i * CHUNK).min(plain.len())..((i + 1) * CHUNK).min(plain.len())];
            let len = self
                .state
                .write_message(self.nonce, piece, &mut out[at..])
                .expect("a chunk under the noise limit");
            self.nonce += 1;
            at += len;
        }
        out.truncate(at);
        out
    }
}

pub struct Opener {
    state: Arc<StatelessTransportState>,
    nonce: u64,
}

impl Opener {
    pub fn open(&mut self, sealed: &[u8]) -> Result<Vec<u8>, SecureError> {
        if sealed.is_empty() {
            return Err(SecureError::Decrypt);
        }
        let mut out = vec![0; sealed.len()];
        let mut at = 0;
        for piece in sealed.chunks(NOISE_MAX) {
            let len = self
                .state
                .read_message(self.nonce, piece, &mut out[at..])
                .map_err(|_| SecureError::Decrypt)?;
            self.nonce += 1;
            at += len;
        }
        out.truncate(at);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(secret: &RelaySecret) -> (RelayKey, Channel, Channel) {
        let (client, first) = ClientHandshake::start();
        let (reply, server) = accept(secret, &first).unwrap();
        let (key, client) = client.finish(&reply).unwrap();
        (key, client, server)
    }

    #[test]
    fn the_client_learns_the_relay_key() {
        let secret = RelaySecret::from_bytes([7; 32]);
        let (key, _, _) = pair(&secret);
        assert_eq!(key, secret.public());
    }

    #[test]
    fn frames_cross_both_ways_in_order() {
        let (_, mut client, mut server) = pair(&RelaySecret::random());
        for text in [&b"hello"[..], b"", b"again"] {
            let sealed = client.seal(text);
            assert_ne!(sealed, text);
            assert_eq!(server.open(&sealed).unwrap(), text);
            assert_eq!(client.open(&server.seal(text)).unwrap(), text);
        }
    }

    #[test]
    fn a_frame_over_one_noise_message_crosses_whole() {
        let (_, mut client, mut server) = pair(&RelaySecret::random());
        let big: Vec<u8> = (0..crate::MAX_FRAME_BYTES).map(|i| i as u8).collect();
        let sealed = client.seal(&big);
        assert_eq!(server.open(&sealed).unwrap(), big);
        assert_eq!(server.open(&client.seal(b"next")).unwrap(), b"next");
    }

    #[test]
    fn a_replayed_or_changed_frame_is_refused() {
        let (_, mut client, mut server) = pair(&RelaySecret::random());
        let sealed = client.seal(b"once");
        assert!(server.open(&sealed).is_ok());
        assert_eq!(server.open(&sealed), Err(SecureError::Decrypt));
        let (_, mut client, mut server) = pair(&RelaySecret::random());
        let mut sealed = client.seal(b"once");
        sealed[0] ^= 1;
        assert_eq!(server.open(&sealed), Err(SecureError::Decrypt));
    }

    #[test]
    fn another_relay_shows_another_key() {
        let (a, _, _) = pair(&RelaySecret::from_bytes([1; 32]));
        let (b, _, _) = pair(&RelaySecret::from_bytes([2; 32]));
        assert_ne!(a, b);
        assert_ne!(a.fingerprint(), b.fingerprint());
    }

    #[test]
    fn a_plain_first_frame_is_not_secure() {
        let hello = crate::ClientMsg::Hello {
            protocol: 1,
            app: "old".into(),
        }
        .encode()
        .unwrap();
        assert!(!is_secure_start(&hello));
        assert!(matches!(
            accept(&RelaySecret::random(), &hello),
            Err(SecureError::NotSecure)
        ));
    }

    fn keyed_pair(
        dial_key: &[u8; 32],
        answer_key: &[u8; 32],
        prologues: (&[u8], &[u8]),
    ) -> Result<(Channel, Channel), SecureError> {
        let (dial, first) = KeyedDial::start(prologues.0)?;
        let (reply, answer) = keyed_answer(prologues.1, answer_key, &first)?;
        Ok((dial.finish(dial_key, &reply)?, answer))
    }

    #[test]
    fn a_shared_key_opens_a_channel_both_ways() {
        let (mut dial, mut answer) = keyed_pair(&[3; 32], &[3; 32], (b"p", b"p")).unwrap();
        let sealed = dial.seal(b"hello");
        assert_ne!(sealed, b"hello");
        assert_eq!(answer.open(&sealed).unwrap(), b"hello");
        assert_eq!(dial.open(&answer.seal(b"back")).unwrap(), b"back");
    }

    #[test]
    fn the_dialer_refuses_an_answer_without_its_key() {
        assert!(matches!(
            keyed_pair(&[3; 32], &[4; 32], (b"p", b"p")),
            Err(SecureError::Handshake)
        ));
        assert!(matches!(
            keyed_pair(&[3; 32], &[3; 32], (b"p", b"q")),
            Err(SecureError::Handshake)
        ));
    }

    #[test]
    fn both_ends_share_a_binding_no_other_session_or_key_has() {
        let (dial, first) = KeyedDial::start(b"p").unwrap();
        let (reply, answer) = keyed_answer(b"p", &[0; 32], &first).unwrap();
        let dial = dial.finish(&[0; 32], &reply).unwrap();
        assert_eq!(dial.binding(), answer.binding());
        assert_ne!(dial.binding(), [0; 32]);
        for sent in [&first, &reply] {
            assert!(!sent.windows(32).any(|w| w == dial.binding()));
        }
        let (other, _) = keyed_pair(&[0; 32], &[0; 32], (b"p", b"p")).unwrap();
        assert_ne!(other.binding(), dial.binding());
    }

    #[test]
    fn a_replayed_first_message_opens_no_channel_the_dialer_can_use() {
        let (dial, first) = KeyedDial::start(b"p").unwrap();
        let (reply, _) = keyed_answer(b"p", &[3; 32], &first).unwrap();
        let mut dial = dial.finish(&[3; 32], &reply).unwrap();
        let sealed = dial.seal(b"once");
        let (_, mut again) = keyed_answer(b"p", &[3; 32], &first).unwrap();
        assert_eq!(again.open(&sealed), Err(SecureError::Decrypt));
    }

    #[test]
    fn fingerprint_reads_back_from_its_text() {
        let print = RelaySecret::from_bytes([9; 32]).public().fingerprint();
        let text = print.to_string();
        assert_eq!(text.len(), 19);
        assert_eq!(text.split(' ').count(), 4);
        assert_eq!(text.parse::<Fingerprint>().unwrap(), print);
        assert_eq!(
            print
                .compact()
                .to_lowercase()
                .parse::<Fingerprint>()
                .unwrap(),
            print
        );
        assert!("K7QM 2XRA".parse::<Fingerprint>().is_err());
    }
}

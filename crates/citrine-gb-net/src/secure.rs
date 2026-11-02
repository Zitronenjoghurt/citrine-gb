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
const NOISE_MAX: usize = 65535;
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

fn builder<'a>() -> Builder<'a> {
    let params: NoiseParams = PATTERN.parse().expect("a valid noise pattern");
    let resolver = FallbackResolver::new(Box::new(RandomResolver), Box::new(DefaultResolver));
    Builder::with_resolver(params, Box::new(resolver))
        .prologue(PROLOGUE)
        .expect("a prologue")
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

pub struct Channel {
    sealer: Sealer,
    opener: Opener,
}

impl Channel {
    fn new(state: HandshakeState) -> Result<Self, SecureError> {
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
        })
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

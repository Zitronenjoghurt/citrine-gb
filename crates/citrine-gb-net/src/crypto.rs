use crate::proto::GameHash;
use crypto_box::aead::Aead;
use crypto_box::{Nonce, SalsaBox, SecretKey};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

const SALT: &[u8] = b"citrine-gb relay id v1";
const AUTH_INFO: &[u8] = b"auth key";
const BOX_INFO: &[u8] = b"x25519 key";
const SAVE_INFO: &[u8] = b"save key";
const SAVE_INDEX_INFO: &[u8] = b"save index";
pub const NONCE_BYTES: usize = 24;
pub const SEAL_OVERHEAD: usize = 16;
const PLAYING_BYTES: usize = 21;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthKey(pub [u8; 32]);

impl AuthKey {
    pub fn hash(&self) -> [u8; 32] {
        Sha256::digest(self.0).into()
    }
}

impl fmt::Debug for AuthKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AuthKey(..)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PublicKey(pub [u8; 32]);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sealed {
    pub nonce: [u8; NONCE_BYTES],
    pub ciphertext: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the message can't be opened with these keys")]
pub struct OpenError;

pub struct Keys {
    auth: AuthKey,
    secret: SecretKey,
    pub(crate) save: [u8; 32],
    pub(crate) save_index: [u8; 32],
}

impl Keys {
    pub(crate) fn derive(relay_id: &[u8; 32]) -> Self {
        let hkdf = Hkdf::<Sha256>::new(Some(SALT), relay_id);
        let mut auth = [0; 32];
        let mut secret = [0; 32];
        hkdf.expand(AUTH_INFO, &mut auth)
            .expect("32 bytes is a valid length");
        hkdf.expand(BOX_INFO, &mut secret)
            .expect("32 bytes is a valid length");
        let mut save = [0; 32];
        let mut save_index = [0; 32];
        hkdf.expand(SAVE_INFO, &mut save)
            .expect("32 bytes is a valid length");
        hkdf.expand(SAVE_INDEX_INFO, &mut save_index)
            .expect("32 bytes is a valid length");
        Self {
            auth: AuthKey(auth),
            secret: SecretKey::from_bytes(secret),
            save,
            save_index,
        }
    }

    pub fn auth_key(&self) -> &AuthKey {
        &self.auth
    }

    pub fn public_key(&self) -> PublicKey {
        PublicKey(self.secret.public_key().to_bytes())
    }

    pub fn seal(&self, to: &PublicKey, plaintext: &[u8]) -> Sealed {
        let mut nonce = [0; NONCE_BYTES];
        getrandom::getrandom(&mut nonce).expect("no system random source");
        self.seal_with_nonce(to, plaintext, nonce)
    }

    fn seal_with_nonce(
        &self,
        to: &PublicKey,
        plaintext: &[u8],
        nonce: [u8; NONCE_BYTES],
    ) -> Sealed {
        let ciphertext = self
            .salsa_box(to)
            .encrypt(Nonce::from_slice(&nonce), plaintext)
            .expect("sealing in memory can't fail");
        Sealed { nonce, ciphertext }
    }

    pub fn open(&self, from: &PublicKey, sealed: &Sealed) -> Result<Vec<u8>, OpenError> {
        self.salsa_box(from)
            .decrypt(
                Nonce::from_slice(&sealed.nonce),
                sealed.ciphertext.as_slice(),
            )
            .map_err(|_| OpenError)
    }

    pub fn seal_playing(&self, to: &PublicKey, playing: Option<GameHash>) -> Sealed {
        let mut plaintext = [0; PLAYING_BYTES];
        if let Some(game) = playing {
            plaintext[0] = 1;
            plaintext[1..].copy_from_slice(&game);
        }
        self.seal(to, &plaintext)
    }

    pub fn open_playing(
        &self,
        from: &PublicKey,
        sealed: &Sealed,
    ) -> Result<Option<GameHash>, OpenError> {
        let plaintext = self.open(from, sealed)?;
        match (plaintext.len(), plaintext.first()) {
            (PLAYING_BYTES, Some(0)) => Ok(None),
            (PLAYING_BYTES, Some(1)) => Ok(Some(plaintext[1..].try_into().map_err(|_| OpenError)?)),
            _ => Err(OpenError),
        }
    }

    fn salsa_box(&self, other: &PublicKey) -> SalsaBox {
        SalsaBox::new(&crypto_box::PublicKey::from_bytes(other.0), &self.secret)
    }
}

impl fmt::Debug for Keys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Keys")
            .field("public_key", &self.public_key())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use crate::RelayId;

    fn id(seed: u8) -> RelayId {
        RelayId::from_bytes([seed; 32])
    }

    #[test]
    fn derivation_is_stable() {
        let a = id(7).keys();
        let b = id(7).keys();
        assert_eq!(a.auth_key(), b.auth_key());
        assert_eq!(a.public_key(), b.public_key());
        assert_eq!(a.auth_key().hash(), b.auth_key().hash());
    }

    #[test]
    fn derived_keys_differ_from_the_id_and_each_other() {
        let relay = id(7);
        let keys = relay.keys();
        assert_ne!(&keys.auth_key().0, relay.as_bytes());
        assert_ne!(keys.auth_key().0, keys.public_key().0);
        assert_ne!(keys.auth_key().hash(), keys.auth_key().0);
        let other = id(8).keys();
        assert_ne!(keys.auth_key(), other.auth_key());
        assert_ne!(keys.public_key(), other.public_key());
    }

    #[test]
    fn auth_key_matches_hkdf_sha256_reference() {
        let keys = RelayId::from_bytes([0; 32]).keys();
        assert_eq!(
            hex(&keys.auth_key().0),
            "32b5b6ef71001ad31d75e816d8aa4c7e82cf26e03decaca3e33c4d50b32a80f4"
        );
        assert_eq!(
            hex(&keys.auth_key().hash()),
            "65c29bca06a68b1dee3de620c766178a53a9333a689b3c1d3540b220791541f7"
        );
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn seal_and_open() {
        let alice = id(1).keys();
        let bob = id(2).keys();
        let sealed = alice.seal(&bob.public_key(), b"hello bob");
        assert_eq!(sealed.ciphertext.len(), 9 + super::SEAL_OVERHEAD);
        assert_ne!(&sealed.ciphertext[..9], b"hello bob");
        assert_eq!(
            bob.open(&alice.public_key(), &sealed).unwrap(),
            b"hello bob"
        );
        assert_eq!(
            alice.open(&bob.public_key(), &sealed).unwrap(),
            b"hello bob"
        );
    }

    #[test]
    fn each_seal_uses_a_fresh_nonce() {
        let alice = id(1).keys();
        let bob = id(2).keys();
        let a = alice.seal(&bob.public_key(), b"same");
        let b = alice.seal(&bob.public_key(), b"same");
        assert_ne!(a.nonce, b.nonce);
        assert_ne!(a.ciphertext, b.ciphertext);
    }

    #[test]
    fn open_refuses_wrong_keys_and_tampering() {
        let alice = id(1).keys();
        let bob = id(2).keys();
        let eve = id(3).keys();
        let sealed = alice.seal(&bob.public_key(), b"secret");
        assert!(eve.open(&alice.public_key(), &sealed).is_err());
        assert!(bob.open(&eve.public_key(), &sealed).is_err());
        let mut flipped = sealed.clone();
        flipped.ciphertext[0] ^= 1;
        assert!(bob.open(&alice.public_key(), &flipped).is_err());
        let mut nonce = sealed.clone();
        nonce.nonce[0] ^= 1;
        assert!(bob.open(&alice.public_key(), &nonce).is_err());
        let mut short = sealed;
        short.ciphertext.truncate(4);
        assert!(bob.open(&alice.public_key(), &short).is_err());
    }

    #[test]
    fn playing_opens_only_for_the_friend_and_hides_the_game() {
        let alice = id(1).keys();
        let bob = id(2).keys();
        let eve = id(3).keys();
        let game = [0x5A; 20];
        let playing = alice.seal_playing(&bob.public_key(), Some(game));
        let nothing = alice.seal_playing(&bob.public_key(), None);
        assert_eq!(playing.ciphertext.len(), nothing.ciphertext.len());
        assert!(!playing.ciphertext.windows(4).any(|w| w == [0x5A; 4]));
        assert_eq!(
            bob.open_playing(&alice.public_key(), &playing),
            Ok(Some(game))
        );
        assert_eq!(bob.open_playing(&alice.public_key(), &nothing), Ok(None));
        assert!(eve.open_playing(&alice.public_key(), &playing).is_err());
        let chat = alice.seal(&bob.public_key(), b"hi");
        assert!(bob.open_playing(&alice.public_key(), &chat).is_err());
    }

    #[test]
    fn seal_with_fixed_nonce_is_deterministic() {
        let alice = id(1).keys();
        let bob = id(2).keys();
        let a = alice.seal_with_nonce(&bob.public_key(), b"x", [9; 24]);
        let b = bob.seal_with_nonce(&alice.public_key(), b"x", [9; 24]);
        assert_eq!(a, b);
    }

    #[test]
    fn debug_hides_secrets() {
        let keys = id(1).keys();
        let text = format!("{keys:?} {:?}", keys.auth_key());
        assert!(text.contains("AuthKey(..)"));
        assert!(!text.contains(&format!("{:?}", keys.auth_key().0)));
    }
}

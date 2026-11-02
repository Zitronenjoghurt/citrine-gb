use crate::chat::{ChatError, pack, unpack};
use crate::crypto::{Keys, NONCE_BYTES};
use crypto_secretbox::aead::{Aead, KeyInit};
use crypto_secretbox::{Nonce, XSalsa20Poly1305};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_SLOT_CHARS: usize = 64;
pub const MAX_LABEL_BYTES: usize = 1024;
pub const MAX_SAVE_BYTES: usize = crate::MAX_FRAME_BYTES - 8 * 1024;
pub const STORAGE_BLOCK: u64 = 4096;

pub fn storage_cost(bytes: u64) -> u64 {
    bytes.max(STORAGE_BLOCK)
}

const MAX_PAYLOAD_BYTES: usize = 32 << 20;
const GAME_INFO: &[u8] = b"save game";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GameKey(pub [u8; 32]);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveInfo {
    pub game: GameKey,
    pub slot: String,
    pub version: u64,
    pub size: u64,
    pub updated: u64,
    pub label: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveLabel {
    pub game: String,
    pub name: String,
    pub device: String,
    pub saved_at: u64,
    pub hash: [u8; 32],
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavePayload {
    pub files: Vec<(String, Vec<u8>)>,
}

impl SavePayload {
    pub fn hash(&self) -> [u8; 32] {
        let bytes = postcard::to_allocvec(self).expect("a payload always serialises");
        Sha256::digest(bytes).into()
    }

    pub fn file(&self, role: &str) -> Option<&[u8]> {
        self.files
            .iter()
            .find(|(r, _)| r == role)
            .map(|(_, data)| data.as_slice())
    }

    pub fn size(&self) -> usize {
        self.files.iter().map(|(_, data)| data.len()).sum()
    }
}

pub fn valid_slot(slot: &str) -> bool {
    !slot.is_empty()
        && slot.len() <= MAX_SLOT_CHARS
        && slot
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
}

impl Keys {
    pub fn game_key(&self, rom_sha256: &[u8]) -> GameKey {
        let hkdf = Hkdf::<Sha256>::new(Some(&self.save_index), rom_sha256);
        let mut key = [0; 32];
        hkdf.expand(GAME_INFO, &mut key)
            .expect("32 bytes is a valid length");
        GameKey(key)
    }

    fn save_box(&self) -> XSalsa20Poly1305 {
        XSalsa20Poly1305::new((&self.save).into())
    }

    fn seal_for_me(&self, plaintext: &[u8]) -> Vec<u8> {
        let mut nonce = [0; NONCE_BYTES];
        getrandom::getrandom(&mut nonce).expect("no system random source");
        let ciphertext = self
            .save_box()
            .encrypt(Nonce::from_slice(&nonce), plaintext)
            .expect("sealing in memory can't fail");
        let mut out = Vec::with_capacity(NONCE_BYTES + ciphertext.len());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ciphertext);
        out
    }

    fn open_for_me(&self, bytes: &[u8]) -> Result<Vec<u8>, ChatError> {
        if bytes.len() < NONCE_BYTES {
            return Err(ChatError::Malformed);
        }
        let (nonce, ciphertext) = bytes.split_at(NONCE_BYTES);
        self.save_box()
            .decrypt(Nonce::from_slice(nonce), ciphertext)
            .map_err(|_| ChatError::Open)
    }

    pub fn seal_save(&self, payload: &SavePayload) -> Vec<u8> {
        let plain = postcard::to_allocvec(payload).expect("a payload always serialises");
        self.seal_for_me(&pack(&plain, true))
    }

    pub fn open_save(&self, bytes: &[u8]) -> Result<SavePayload, ChatError> {
        let packed = self.open_for_me(bytes)?;
        let plain = unpack(&packed, MAX_PAYLOAD_BYTES)?;
        match postcard::take_from_bytes(&plain) {
            Ok((payload, [])) => Ok(payload),
            _ => Err(ChatError::Malformed),
        }
    }

    pub fn seal_label(&self, label: &SaveLabel) -> Vec<u8> {
        let plain = postcard::to_allocvec(label).expect("a label always serialises");
        self.seal_for_me(&plain)
    }

    pub fn open_label(&self, bytes: &[u8]) -> Result<SaveLabel, ChatError> {
        let plain = self.open_for_me(bytes)?;
        match postcard::take_from_bytes(&plain) {
            Ok((label, [])) => Ok(label),
            _ => Err(ChatError::Malformed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RelayId;

    fn payload() -> SavePayload {
        SavePayload {
            files: vec![
                ("sav".into(), vec![0xFF; 32 * 1024]),
                ("meta".into(), 1_700_000_000u64.to_le_bytes().to_vec()),
            ],
        }
    }

    #[test]
    fn a_save_opens_only_with_the_same_relay_id() {
        let mine = RelayId::from_bytes([1; 32]);
        let sealed = mine.keys().seal_save(&payload());
        assert_eq!(mine.keys().open_save(&sealed), Ok(payload()));
        let other = RelayId::from_bytes([2; 32]).keys();
        assert_eq!(other.open_save(&sealed), Err(ChatError::Open));
    }

    #[test]
    fn a_save_is_compressed_before_it_is_sealed() {
        let keys = RelayId::from_bytes([1; 32]).keys();
        let sealed = keys.seal_save(&payload());
        assert!(sealed.len() < 1024, "sealed {} bytes", sealed.len());
        assert!(!sealed.windows(64).any(|w| w.iter().all(|&b| b == 0xFF)));
    }

    #[test]
    fn the_game_key_is_the_same_on_every_device_and_hides_the_rom() {
        let rom = [0x5A; 32];
        let a = RelayId::from_bytes([1; 32]).keys().game_key(&rom);
        let b = RelayId::from_bytes([1; 32]).keys().game_key(&rom);
        let other = RelayId::from_bytes([2; 32]).keys().game_key(&rom);
        assert_eq!(a, b);
        assert_ne!(a, other);
        assert_ne!(a.0, rom);
        let keys = RelayId::from_bytes([1; 32]).keys();
        assert_ne!(keys.game_key(&[0x5B; 32]), a);
    }

    #[test]
    fn labels_round_trip_sealed() {
        let keys = RelayId::from_bytes([1; 32]).keys();
        let label = SaveLabel {
            game: "Tobu Tobu Girl".into(),
            name: "Slot 2".into(),
            device: "Firefox on Android".into(),
            saved_at: 1_700_000_000,
            hash: payload().hash(),
        };
        let sealed = keys.seal_label(&label);
        assert!(!sealed.windows(4).any(|w| w == b"Tobu"));
        assert_eq!(keys.open_label(&sealed), Ok(label));
        assert!(keys.open_label(&sealed[..10]).is_err());
    }

    #[test]
    fn slot_names_are_short_and_plain() {
        assert!(valid_slot("battery"));
        assert!(valid_slot("slot-3"));
        assert!(valid_slot("auto-1700000000-ab12cd34"));
        assert!(!valid_slot(""));
        assert!(!valid_slot("slot/3"));
        assert!(!valid_slot(&"a".repeat(MAX_SLOT_CHARS + 1)));
    }

    #[test]
    fn the_hash_follows_the_bytes() {
        let mut changed = payload();
        changed.files[0].1[0] = 0;
        assert_ne!(changed.hash(), payload().hash());
        assert_eq!(payload().hash(), payload().hash());
        assert_eq!(payload().file("meta").map(<[u8]>::len), Some(8));
        assert_eq!(payload().size(), 32 * 1024 + 8);
    }
}

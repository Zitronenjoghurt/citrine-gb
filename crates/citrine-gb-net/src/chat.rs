use crate::crypto::{Keys, NONCE_BYTES, PublicKey, Sealed};
use crate::proto::FileId;
use serde::{Deserialize, Serialize};
use std::io::Read;

pub const MAX_TEXT_CHARS: usize = 4000;
pub const MAX_RECEIPT_IDS: usize = 300;
pub const MAX_PALETTE_BYTES: usize = 1024;
pub const MAX_UNPACKED_BYTES: usize = 64 << 20;
const MAX_ENVELOPE_BYTES: usize = 1 << 20;
const RAW: u8 = 0;
const ZSTD: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ShareKind {
    Screenshot,
    Recording,
    Replay,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Share {
    pub file: FileId,
    pub kind: ShareKind,
    pub size: u64,
    pub game: String,
    pub rom_sha256: Option<String>,
    pub created_at: u64,
    pub width: u32,
    pub height: u32,
    pub length_ms: Option<u64>,
    pub preview: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Body {
    Text(String),
    Share(Share),
    Receipt(Receipt),
    Palette(String),
    Edit(Edit),
    Delete(u64),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edit {
    pub id: u64,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub delivered: Vec<u64>,
    pub read: Vec<u64>,
}

impl Receipt {
    pub fn is_empty(&self) -> bool {
        self.delivered.is_empty() && self.read.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub id: u64,
    pub body: Body,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ChatError {
    #[error("the message can't be opened with these keys")]
    Open,
    #[error("the message is malformed")]
    Malformed,
    #[error("the message unpacks to more than the limit")]
    TooLarge,
}

pub fn pack(data: &[u8], compress: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 1);
    if compress {
        let packed =
            ruzstd::encoding::compress_to_vec(data, ruzstd::encoding::CompressionLevel::Fastest);
        if packed.len() < data.len() {
            out.push(ZSTD);
            out.extend_from_slice(&packed);
            return out;
        }
    }
    out.push(RAW);
    out.extend_from_slice(data);
    out
}

pub fn unpack(bytes: &[u8], limit: usize) -> Result<Vec<u8>, ChatError> {
    let (&format, data) = bytes.split_first().ok_or(ChatError::Malformed)?;
    match format {
        RAW if data.len() <= limit => Ok(data.to_vec()),
        RAW => Err(ChatError::TooLarge),
        ZSTD => {
            let mut source = data;
            let decoder = ruzstd::decoding::StreamingDecoder::new(&mut source)
                .map_err(|_| ChatError::Malformed)?;
            let mut out = Vec::new();
            decoder
                .take(limit as u64 + 1)
                .read_to_end(&mut out)
                .map_err(|_| ChatError::Malformed)?;
            if out.len() > limit {
                return Err(ChatError::TooLarge);
            }
            Ok(out)
        }
        _ => Err(ChatError::Malformed),
    }
}

impl Sealed {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(NONCE_BYTES + self.ciphertext.len());
        out.extend_from_slice(&self.nonce);
        out.extend_from_slice(&self.ciphertext);
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < NONCE_BYTES {
            return None;
        }
        let (nonce, ciphertext) = bytes.split_at(NONCE_BYTES);
        Some(Self {
            nonce: nonce.try_into().ok()?,
            ciphertext: ciphertext.to_vec(),
        })
    }
}

impl Envelope {
    pub fn new(body: Body) -> Self {
        let mut id = [0; 8];
        getrandom::getrandom(&mut id).expect("no system random source");
        Self {
            id: u64::from_le_bytes(id),
            body,
        }
    }

    pub fn packed(&self) -> Vec<u8> {
        let plain = postcard::to_allocvec(self).expect("an envelope always serialises");
        let compress = matches!(self.body, Body::Text(_) | Body::Edit(_));
        pack(&plain, compress)
    }

    pub fn unpacked(bytes: &[u8]) -> Result<Self, ChatError> {
        let plain = unpack(bytes, MAX_ENVELOPE_BYTES)?;
        match postcard::take_from_bytes(&plain) {
            Ok((envelope, [])) => Ok(envelope),
            _ => Err(ChatError::Malformed),
        }
    }
}

impl Keys {
    pub fn seal_envelope(&self, to: &PublicKey, envelope: &Envelope) -> Sealed {
        self.seal(to, &envelope.packed())
    }

    pub fn open_envelope(&self, from: &PublicKey, sealed: &Sealed) -> Result<Envelope, ChatError> {
        let plain = self.open(from, sealed).map_err(|_| ChatError::Open)?;
        Envelope::unpacked(&plain)
    }

    pub fn seal_file(&self, to: &PublicKey, data: &[u8], compress: bool) -> Vec<u8> {
        self.seal(to, &pack(data, compress)).to_bytes()
    }

    pub fn open_file(&self, from: &PublicKey, bytes: &[u8]) -> Result<Vec<u8>, ChatError> {
        let sealed = Sealed::from_bytes(bytes).ok_or(ChatError::Malformed)?;
        let plain = self.open(from, &sealed).map_err(|_| ChatError::Open)?;
        unpack(&plain, MAX_UNPACKED_BYTES)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RelayId;

    fn keys(seed: u8) -> Keys {
        RelayId::from_bytes([seed; 32]).keys()
    }

    fn share() -> Share {
        Share {
            file: FileId([7; 16]),
            kind: ShareKind::Replay,
            size: 1234,
            game: "Tobu Tobu Girl".into(),
            rom_sha256: Some("ab".repeat(32)),
            created_at: 1_761_800_000,
            width: 0,
            height: 0,
            length_ms: None,
            preview: Vec::new(),
        }
    }

    #[test]
    fn text_and_shares_round_trip_sealed() {
        let alice = keys(1);
        let bob = keys(2);
        let receipt = Body::Receipt(Receipt {
            delivered: vec![1, 2],
            read: vec![u64::MAX],
        });
        let bodies = [
            Body::Text("hi 🍋 hi".into()),
            Body::Share(share()),
            receipt,
            Body::Palette(r#"{"name":"Lemon","colors":[[255,255,204]]}"#.into()),
            Body::Edit(Edit {
                id: 9,
                text: "hi again".into(),
            }),
            Body::Delete(9),
        ];
        for body in bodies {
            let envelope = Envelope::new(body);
            let sealed = alice.seal_envelope(&bob.public_key(), &envelope);
            assert_eq!(
                bob.open_envelope(&alice.public_key(), &sealed),
                Ok(envelope)
            );
            assert_eq!(
                keys(3).open_envelope(&alice.public_key(), &sealed),
                Err(ChatError::Open)
            );
        }
    }

    #[test]
    fn an_older_app_drops_a_receipt_as_malformed() {
        #[allow(dead_code)]
        #[derive(Debug, Serialize, Deserialize)]
        enum OlderBody {
            Text(String),
            Share(Share),
        }
        #[allow(dead_code)]
        #[derive(Debug, Serialize, Deserialize)]
        struct OlderEnvelope {
            id: u64,
            body: OlderBody,
        }
        let text = Envelope::new(Body::Text("hi".into()));
        let plain = postcard::to_allocvec(&text).unwrap();
        assert!(postcard::from_bytes::<OlderEnvelope>(&plain).is_ok());
        let receipt = Envelope::new(Body::Receipt(Receipt {
            delivered: vec![7],
            read: Vec::new(),
        }));
        let plain = postcard::to_allocvec(&receipt).unwrap();
        assert!(postcard::from_bytes::<OlderEnvelope>(&plain).is_err());
    }

    #[test]
    fn an_older_app_still_reads_texts_after_the_new_kinds() {
        #[allow(dead_code)]
        #[derive(Debug, Serialize, Deserialize)]
        enum OlderBody {
            Text(String),
            Share(Share),
            Receipt(Receipt),
        }
        #[allow(dead_code)]
        #[derive(Debug, Serialize, Deserialize)]
        struct OlderEnvelope {
            id: u64,
            body: OlderBody,
        }
        let text = postcard::to_allocvec(&Envelope::new(Body::Text("hi".into()))).unwrap();
        assert!(postcard::from_bytes::<OlderEnvelope>(&text).is_ok());
        for body in [Body::Palette("{}".into()), Body::Delete(1)] {
            let plain = postcard::to_allocvec(&Envelope::new(body)).unwrap();
            assert!(postcard::from_bytes::<OlderEnvelope>(&plain).is_err());
        }
    }

    #[test]
    fn each_envelope_gets_its_own_id() {
        let a = Envelope::new(Body::Text("x".into()));
        let b = Envelope::new(Body::Text("x".into()));
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn long_text_shrinks_and_short_text_stays_raw() {
        let long = "Let's meet at the lemon tree after the boss fight. ".repeat(20);
        let packed = Envelope::new(Body::Text(long.clone())).packed();
        assert_eq!(packed[0], ZSTD);
        assert!(packed.len() < long.len() / 3, "{} bytes", packed.len());
        let short = Envelope::new(Body::Text("hi".into())).packed();
        assert_eq!(short[0], RAW);
    }

    #[test]
    fn files_round_trip_and_only_the_pair_can_open_them() {
        let alice = keys(1);
        let bob = keys(2);
        let json = br#"{"rom_sha256":"abc","events":[]}"#.repeat(100);
        let sealed = alice.seal_file(&bob.public_key(), &json, true);
        assert!(sealed.len() < json.len() / 4);
        assert!(!sealed.windows(9).any(|w| w == b"rom_sha25"));
        assert_eq!(bob.open_file(&alice.public_key(), &sealed).unwrap(), json);
        assert_eq!(alice.open_file(&bob.public_key(), &sealed).unwrap(), json);
        assert_eq!(
            keys(3).open_file(&alice.public_key(), &sealed),
            Err(ChatError::Open)
        );
        assert_eq!(
            bob.open_file(&alice.public_key(), &sealed[..10]),
            Err(ChatError::Malformed)
        );
    }

    #[test]
    fn random_bytes_are_stored_as_they_are() {
        let mut noise = vec![0u8; 4096];
        getrandom::getrandom(&mut noise).unwrap();
        let packed = pack(&noise, true);
        assert_eq!(packed[0], RAW);
        assert_eq!(packed.len(), noise.len() + 1);
        assert_eq!(unpack(&packed, noise.len()).unwrap(), noise);
    }

    #[test]
    fn unpacking_stops_at_the_limit() {
        let zeros = vec![0u8; 100_000];
        let packed = pack(&zeros, true);
        assert!(packed.len() < 1000);
        assert_eq!(unpack(&packed, 99_999), Err(ChatError::TooLarge));
        assert_eq!(unpack(&packed, 100_000).unwrap().len(), 100_000);
        assert_eq!(unpack(&[RAW, 1, 2, 3], 2), Err(ChatError::TooLarge));
        assert_eq!(unpack(&[9, 1], 10), Err(ChatError::Malformed));
        assert_eq!(unpack(&[], 10), Err(ChatError::Malformed));
        assert_eq!(unpack(&[ZSTD, 1, 2, 3], 10), Err(ChatError::Malformed));
    }

    #[test]
    fn sealed_bytes_round_trip() {
        let sealed = keys(1).seal(&keys(2).public_key(), b"abc");
        assert_eq!(Sealed::from_bytes(&sealed.to_bytes()), Some(sealed));
        assert_eq!(Sealed::from_bytes(&[0; 5]), None);
    }
}

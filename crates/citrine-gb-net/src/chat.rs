use crate::crypto::{Keys, NONCE_BYTES, PublicKey, Sealed};
use crate::proto::FileId;
use serde::{Deserialize, Serialize};
use std::io::Read;

pub const MAX_TEXT_CHARS: usize = 4000;
pub const MAX_RECEIPT_IDS: usize = 300;
pub const MAX_PALETTE_BYTES: usize = 1024;
pub const MAX_UNPACKED_BYTES: usize = 64 << 20;
pub const MAX_QUOTE_CHARS: usize = 120;
pub const MAX_TYPING_CHARS: usize = 32;
pub const CHAT_EXTRAS_VERSION: u8 = 1;
const MAX_ENVELOPE_BYTES: usize = 1 << 20;
const MAX_EXTRAS_BYTES: usize = 4096;
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
    TypingText(String),
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
pub struct Quote {
    pub id: u64,
    pub by_sender: bool,
    pub line: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    pub id: u64,
    pub body: Body,
    pub reply: Option<Quote>,
}

#[derive(Serialize, Deserialize)]
struct Extras {
    reply: Option<Quote>,
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

const MAX_WINDOW: u64 = 8 << 20;

pub fn unpack(bytes: &[u8], limit: usize) -> Result<Vec<u8>, ChatError> {
    unpack_with_rest(bytes, limit).map(|(out, _)| out)
}

fn unpack_with_rest(bytes: &[u8], limit: usize) -> Result<(Vec<u8>, &[u8]), ChatError> {
    let (&format, data) = bytes.split_first().ok_or(ChatError::Malformed)?;
    match format {
        RAW if data.len() <= limit => Ok((data.to_vec(), &[])),
        RAW => Err(ChatError::TooLarge),
        ZSTD => {
            let mut source = data;
            let decoder = ruzstd::decoding::StreamingDecoder::new_with_max_window_size(
                &mut source,
                MAX_WINDOW,
            )
            .map_err(|_| ChatError::Malformed)?;
            let mut out = Vec::new();
            decoder
                .take(limit as u64 + 1)
                .read_to_end(&mut out)
                .map_err(|_| ChatError::Malformed)?;
            if out.len() > limit {
                return Err(ChatError::TooLarge);
            }
            Ok((out, source))
        }
        _ => Err(ChatError::Malformed),
    }
}

fn zstd_frame(data: &[u8]) -> Vec<u8> {
    let mut out = vec![ZSTD];
    out.extend(ruzstd::encoding::compress_to_vec(
        data,
        ruzstd::encoding::CompressionLevel::Fastest,
    ));
    out
}

pub fn quote_line(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    line.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_QUOTE_CHARS)
        .collect::<String>()
        .trim()
        .to_owned()
}

pub fn typing_text(text: &str) -> String {
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|word| !word.is_empty())
        .collect();
    let joined = words.join(" ");
    let cut: String = joined
        .trim_end_matches(['.', '…', ' '])
        .chars()
        .take(MAX_TYPING_CHARS)
        .collect();
    cut.trim_end_matches(['.', '…', ' ']).to_owned()
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
            reply: None,
        }
    }

    pub fn replying(mut self, reply: Option<Quote>) -> Self {
        self.reply = reply;
        self
    }

    pub fn packed(&self) -> Vec<u8> {
        let plain =
            postcard::to_allocvec(&(self.id, &self.body)).expect("an envelope always serialises");
        if self.reply.is_none() {
            let compress = matches!(self.body, Body::Text(_) | Body::Edit(_));
            return pack(&plain, compress);
        }
        let extras = Extras {
            reply: self.reply.clone(),
        };
        let mut out = zstd_frame(&plain);
        out.push(CHAT_EXTRAS_VERSION);
        out.extend(postcard::to_allocvec(&extras).expect("extras always serialise"));
        out
    }

    pub fn unpacked(bytes: &[u8]) -> Result<Self, ChatError> {
        let (plain, rest) = unpack_with_rest(bytes, MAX_ENVELOPE_BYTES)?;
        let Ok(((id, body), [])) = postcard::take_from_bytes::<(u64, Body)>(&plain) else {
            return Err(ChatError::Malformed);
        };
        let reply = match rest.split_first() {
            Some((&CHAT_EXTRAS_VERSION, extras)) if extras.len() <= MAX_EXTRAS_BYTES => {
                postcard::from_bytes::<Extras>(extras)
                    .ok()
                    .and_then(|extras| extras.reply)
            }
            _ => None,
        };
        let reply = reply.map(|quote| Quote {
            line: quote_line(&quote.line),
            ..quote
        });
        Ok(Self { id, body, reply })
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

    fn raw_block_frame(window: u8) -> Vec<u8> {
        [
            &[ZSTD, 0x28, 0xb5, 0x2f, 0xfd, 0x00, window, 0x29, 0x00, 0x00][..],
            b"hello",
        ]
        .concat()
    }

    #[test]
    fn a_frame_that_claims_a_huge_window_is_refused() {
        assert_eq!(unpack(&raw_block_frame(0x00), 1 << 20).unwrap(), b"hello");
        assert!(unpack(&raw_block_frame(0x80), 1 << 20).is_err());
    }

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
            Body::TypingText("is meowing".into()),
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
        let plain = unpack(&text.packed(), MAX_ENVELOPE_BYTES).unwrap();
        assert!(postcard::from_bytes::<OlderEnvelope>(&plain).is_ok());
        let receipt = Envelope::new(Body::Receipt(Receipt {
            delivered: vec![7],
            read: Vec::new(),
        }));
        let plain = unpack(&receipt.packed(), MAX_ENVELOPE_BYTES).unwrap();
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
        let text = unpack(
            &Envelope::new(Body::Text("hi".into())).packed(),
            MAX_ENVELOPE_BYTES,
        )
        .unwrap();
        assert!(postcard::from_bytes::<OlderEnvelope>(&text).is_ok());
        for body in [Body::Palette("{}".into()), Body::Delete(1)] {
            let plain = unpack(&Envelope::new(body).packed(), MAX_ENVELOPE_BYTES).unwrap();
            assert!(postcard::from_bytes::<OlderEnvelope>(&plain).is_err());
        }
    }

    #[allow(dead_code)]
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    enum ShippedBody {
        Text(String),
        Share(Share),
        Receipt(Receipt),
        Palette(String),
        Edit(Edit),
        Delete(u64),
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct ShippedEnvelope {
        id: u64,
        body: ShippedBody,
    }

    fn shipped_open(bytes: &[u8]) -> Result<ShippedEnvelope, ChatError> {
        let plain = unpack(bytes, MAX_ENVELOPE_BYTES)?;
        match postcard::take_from_bytes(&plain) {
            Ok((envelope, [])) => Ok(envelope),
            _ => Err(ChatError::Malformed),
        }
    }

    fn quote(line: &str) -> Quote {
        Quote {
            id: 41,
            by_sender: false,
            line: line.into(),
        }
    }

    #[test]
    fn a_reply_round_trips_sealed_with_its_quote() {
        let alice = keys(1);
        let bob = keys(2);
        for text in ["ok", &"long reply ".repeat(400)] {
            let envelope = Envelope::new(Body::Text(text.into()))
                .replying(Some(quote("did you beat the castle?")));
            let sealed = alice.seal_envelope(&bob.public_key(), &envelope);
            assert_eq!(
                bob.open_envelope(&alice.public_key(), &sealed),
                Ok(envelope)
            );
        }
    }

    #[test]
    fn the_shipped_app_reads_a_reply_as_a_plain_message() {
        for text in ["ok", &"long reply ".repeat(400)] {
            let envelope = Envelope::new(Body::Text(text.into()))
                .replying(Some(quote("did you beat the castle?")));
            assert_eq!(
                shipped_open(&envelope.packed()),
                Ok(ShippedEnvelope {
                    id: envelope.id,
                    body: ShippedBody::Text(text.into()),
                })
            );
        }
    }

    #[test]
    fn a_message_without_a_reply_packs_as_the_shipped_app_does() {
        for text in ["hi", &"Let's meet at the lemon tree. ".repeat(20)] {
            let envelope = Envelope::new(Body::Text(text.into()));
            let shipped = ShippedEnvelope {
                id: envelope.id,
                body: ShippedBody::Text(text.into()),
            };
            let plain = postcard::to_allocvec(&shipped).unwrap();
            assert_eq!(envelope.packed(), pack(&plain, true));
        }
    }

    #[test]
    fn the_shipped_app_drops_a_typing_text_quietly() {
        let envelope = Envelope::new(Body::TypingText("is meowing".into()));
        assert_eq!(shipped_open(&envelope.packed()), Err(ChatError::Malformed));
        assert_eq!(Envelope::unpacked(&envelope.packed()), Ok(envelope));
    }

    #[test]
    fn a_reply_quote_from_the_wire_is_one_short_line() {
        let long = format!("{}\nsecond line", "a".repeat(500));
        let envelope = Envelope::new(Body::Text("ok".into())).replying(Some(quote(&long)));
        let opened = Envelope::unpacked(&envelope.packed()).unwrap();
        assert_eq!(opened.reply.unwrap().line, "a".repeat(MAX_QUOTE_CHARS));
    }

    #[test]
    fn unknown_extras_are_left_out() {
        let envelope = Envelope::new(Body::Text("ok".into())).replying(Some(quote("hi")));
        let mut bytes = envelope.packed();
        let at = bytes.len()
            - 1
            - postcard::to_allocvec(&Extras {
                reply: Some(quote("hi")),
            })
            .unwrap()
            .len();
        bytes[at] = CHAT_EXTRAS_VERSION + 1;
        assert_eq!(
            Envelope::unpacked(&bytes),
            Ok(Envelope {
                reply: None,
                ..envelope
            })
        );
    }

    #[test]
    fn quote_lines_take_the_first_words_on_one_line() {
        assert_eq!(quote_line("\n  see you\tthere \nbye"), "see you there");
        assert_eq!(quote_line(""), "");
    }

    #[test]
    fn typing_texts_are_one_short_line_without_dots() {
        assert_eq!(typing_text("is meowing..."), "is meowing");
        assert_eq!(typing_text("  is\nmeowing\u{7}  …"), "is meowing");
        assert_eq!(typing_text("   "), "");
        assert_eq!(
            typing_text(&"x".repeat(40)).chars().count(),
            MAX_TYPING_CHARS
        );
        assert_eq!(
            typing_text("is purring very loudly at the door..."),
            "is purring very loudly at the do"
        );
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

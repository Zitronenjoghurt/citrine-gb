use crate::crypto::{NONCE_BYTES, OpenError, SEAL_OVERHEAD};
use crypto_secretbox::aead::{Aead, KeyInit};
use crypto_secretbox::{Nonce, XSalsa20Poly1305};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::io::Read;
use std::path::Path;

pub const ARCHIVE_PROTOCOL: u16 = 11;
pub const ARCHIVE_PART_BYTES: usize = 256 * 1024;
pub const ARCHIVE_SEALED_PART_BYTES: usize = ARCHIVE_PART_BYTES + SEAL_OVERHEAD;
pub const ARCHIVE_PREFIX_BYTES: usize = 16;
const ARCHIVE_WINDOW: u64 = 1 << 25;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ArchiveId(pub [u8; 16]);

impl fmt::Display for ArchiveId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl ArchiveId {
    pub fn parse(hex: &str) -> Option<Self> {
        if hex.len() != 32 || !hex.is_ascii() {
            return None;
        }
        let mut id = [0; 16];
        for (i, b) in id.iter_mut().enumerate() {
            *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
        }
        Some(Self(id))
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveKey(pub [u8; 32]);

impl fmt::Debug for ArchiveKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ArchiveKey(..)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ArchiveConsole {
    GameBoy,
    GameBoyColor,
    GameBoyAdvance,
    Ds,
    Other,
}

impl ArchiveConsole {
    pub const ALL: [Self; 5] = [
        Self::GameBoy,
        Self::GameBoyColor,
        Self::GameBoyAdvance,
        Self::Ds,
        Self::Other,
    ];

    pub fn of(file_name: &str) -> Self {
        let ext = Path::new(file_name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        match ext.as_str() {
            "gb" => Self::GameBoy,
            "gbc" | "cgb" => Self::GameBoyColor,
            "gba" | "agb" => Self::GameBoyAdvance,
            "nds" | "dsi" => Self::Ds,
            _ => Self::Other,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::GameBoy => "gb",
            Self::GameBoyColor => "gbc",
            Self::GameBoyAdvance => "gba",
            Self::Ds => "nds",
            Self::Other => "bin",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::GameBoy => "gb",
            Self::GameBoyColor => "gbc",
            Self::GameBoyAdvance => "gba",
            Self::Ds => "ds",
            Self::Other => "other",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::GameBoy => "Game Boy",
            Self::GameBoyColor => "Game Boy Color",
            Self::GameBoyAdvance => "Game Boy Advance",
            Self::Ds => "Nintendo DS",
            Self::Other => "Other",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::GameBoy => "GB",
            Self::GameBoyColor => "GBC",
            Self::GameBoyAdvance => "GBA",
            Self::Ds => "DS",
            Self::Other => "Other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UnpackError {
    #[error("the download is damaged")]
    Damaged,
    #[error("the download is not this game")]
    Mismatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveGame {
    pub id: ArchiveId,
    pub name: String,
    pub console: ArchiveConsole,
    pub size: u64,
    pub download: u64,
    pub sha256: [u8; 32],
    pub sha1: [u8; 20],
    pub crc32: u32,
}

impl ArchiveGame {
    pub fn file_name(&self) -> String {
        format!("{}.{}", self.name, self.console.extension())
    }

    pub fn unpack(&self, packed: &[u8]) -> Result<Vec<u8>, UnpackError> {
        let mut source = packed;
        let decoder = ruzstd::decoding::StreamingDecoder::new_with_max_window_size(
            &mut source,
            ARCHIVE_WINDOW,
        )
        .map_err(|_| UnpackError::Damaged)?;
        let mut rom = Vec::new();
        decoder
            .take(self.size + 1)
            .read_to_end(&mut rom)
            .map_err(|_| UnpackError::Damaged)?;
        if rom.len() as u64 != self.size || Sha256::digest(&rom)[..] != self.sha256 {
            return Err(UnpackError::Mismatch);
        }
        Ok(rom)
    }
}

pub fn archive_parts(download: u64) -> u32 {
    download.div_ceil(ARCHIVE_SEALED_PART_BYTES as u64) as u32
}

fn part_nonce(prefix: &[u8; ARCHIVE_PREFIX_BYTES], index: u32) -> [u8; NONCE_BYTES] {
    let mut nonce = [0; NONCE_BYTES];
    nonce[..ARCHIVE_PREFIX_BYTES].copy_from_slice(prefix);
    nonce[ARCHIVE_PREFIX_BYTES..].copy_from_slice(&u64::from(index).to_le_bytes());
    nonce
}

impl ArchiveKey {
    pub fn random() -> Self {
        let mut key = [0; 32];
        getrandom::getrandom(&mut key).expect("no system random source");
        Self(key)
    }

    fn cipher(&self) -> XSalsa20Poly1305 {
        XSalsa20Poly1305::new((&self.0).into())
    }

    pub fn seal_part(
        &self,
        prefix: &[u8; ARCHIVE_PREFIX_BYTES],
        index: u32,
        part: &[u8],
    ) -> Vec<u8> {
        self.cipher()
            .encrypt(Nonce::from_slice(&part_nonce(prefix, index)), part)
            .expect("sealing in memory can't fail")
    }

    pub fn open_part(
        &self,
        prefix: &[u8; ARCHIVE_PREFIX_BYTES],
        index: u32,
        sealed: &[u8],
    ) -> Result<Vec<u8>, OpenError> {
        self.cipher()
            .decrypt(Nonce::from_slice(&part_nonce(prefix, index)), sealed)
            .map_err(|_| OpenError)
    }

    pub fn seal(&self, plain: &[u8]) -> Vec<u8> {
        let mut nonce = [0; NONCE_BYTES];
        getrandom::getrandom(&mut nonce).expect("no system random source");
        let mut out = nonce.to_vec();
        out.extend(
            self.cipher()
                .encrypt(Nonce::from_slice(&nonce), plain)
                .expect("sealing in memory can't fail"),
        );
        out
    }

    pub fn open(&self, sealed: &[u8]) -> Result<Vec<u8>, OpenError> {
        if sealed.len() < NONCE_BYTES {
            return Err(OpenError);
        }
        let (nonce, ciphertext) = sealed.split_at(NONCE_BYTES);
        self.cipher()
            .decrypt(Nonce::from_slice(nonce), ciphertext)
            .map_err(|_| OpenError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_part_opens_only_at_its_own_index() {
        let key = ArchiveKey([5; 32]);
        let prefix = [9; ARCHIVE_PREFIX_BYTES];
        let sealed = key.seal_part(&prefix, 3, b"part three");
        assert_eq!(sealed.len(), 10 + SEAL_OVERHEAD);
        assert_eq!(key.open_part(&prefix, 3, &sealed).unwrap(), b"part three");
        assert!(key.open_part(&prefix, 4, &sealed).is_err());
        assert!(
            key.open_part(&[8; ARCHIVE_PREFIX_BYTES], 3, &sealed)
                .is_err()
        );
        assert!(ArchiveKey([6; 32]).open_part(&prefix, 3, &sealed).is_err());
    }

    #[test]
    fn a_sealed_blob_opens_with_its_key_only() {
        let key = ArchiveKey([1; 32]);
        let sealed = key.seal(b"Pokemon Platinum");
        assert_eq!(key.open(&sealed).unwrap(), b"Pokemon Platinum");
        assert!(ArchiveKey([2; 32]).open(&sealed).is_err());
        assert!(key.open(&sealed[..10]).is_err());
        assert_ne!(key.seal(b"Pokemon Platinum"), sealed);
    }

    #[test]
    fn parts_count_sealed_bytes() {
        assert_eq!(archive_parts(0), 0);
        assert_eq!(archive_parts(1), 1);
        assert_eq!(archive_parts(ARCHIVE_SEALED_PART_BYTES as u64), 1);
        assert_eq!(archive_parts(ARCHIVE_SEALED_PART_BYTES as u64 + 1), 2);
    }

    #[test]
    fn ids_print_and_parse_as_hex() {
        let id = ArchiveId([0xAB; 16]);
        assert_eq!(id.to_string(), "ab".repeat(16));
        assert_eq!(ArchiveId::parse(&id.to_string()), Some(id));
        assert_eq!(ArchiveId::parse("ab"), None);
        assert_eq!(ArchiveId::parse(&"zz".repeat(16)), None);
    }

    fn game_of(rom: &[u8]) -> ArchiveGame {
        ArchiveGame {
            id: ArchiveId([1; 16]),
            name: "Tetris".into(),
            console: ArchiveConsole::GameBoy,
            size: rom.len() as u64,
            download: 0,
            sha256: Sha256::digest(rom).into(),
            sha1: [0; 20],
            crc32: 0,
        }
    }

    #[test]
    fn a_packed_game_unpacks_to_its_own_bytes() {
        let rom: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let packed = ruzstd::encoding::compress_to_vec(
            &rom[..],
            ruzstd::encoding::CompressionLevel::Fastest,
        );
        assert_eq!(game_of(&rom).unpack(&packed).unwrap(), rom);
        let mut other = rom.clone();
        other[5] ^= 1;
        assert_eq!(game_of(&other).unpack(&packed), Err(UnpackError::Mismatch));
        assert_eq!(
            game_of(&rom).unpack(&packed[..packed.len() / 2]),
            Err(UnpackError::Damaged)
        );
        assert_eq!(game_of(&rom).file_name(), "Tetris.gb");
    }

    #[test]
    fn consoles_come_from_the_file_name() {
        assert_eq!(
            ArchiveConsole::of("Tetris (World).gb"),
            ArchiveConsole::GameBoy
        );
        assert_eq!(
            ArchiveConsole::of("Zelda.GBC"),
            ArchiveConsole::GameBoyColor
        );
        assert_eq!(
            ArchiveConsole::of("emerald.gba"),
            ArchiveConsole::GameBoyAdvance
        );
        assert_eq!(ArchiveConsole::of("platinum.nds"), ArchiveConsole::Ds);
        assert_eq!(ArchiveConsole::of("notes.txt"), ArchiveConsole::Other);
        assert_eq!(ArchiveConsole::of("no extension"), ArchiveConsole::Other);
    }

    #[test]
    fn keys_never_print() {
        assert_eq!(format!("{:?}", ArchiveKey([7; 32])), "ArchiveKey(..)");
    }
}

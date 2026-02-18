use crate::disassembler::Disassembly;
use std::path::Path;

const NINTENDO_LOGO: [u8; 48] = [
    0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0C, 0x00, 0x0D,
    0x00, 0x08, 0x11, 0x1F, 0x88, 0x89, 0x00, 0x0E, 0xDC, 0xCC, 0x6E, 0xE6, 0xDD, 0xDD, 0xD9, 0x99,
    0xBB, 0xBB, 0x67, 0x63, 0x6E, 0x0E, 0xEC, 0xCC, 0xDD, 0xDC, 0x99, 0x9F, 0xBB, 0xB9, 0x33, 0x3E,
];

#[derive(Debug)]
pub struct Rom {
    pub header: RomHeader,
    pub data: Vec<u8>,
}

impl Rom {
    pub fn new(data: &[u8]) -> Self {
        Self {
            header: RomHeader::new(data),
            data: data.to_vec(),
        }
    }

    pub fn from_file(path: &Path) -> Result<Self, std::io::Error> {
        let data = std::fs::read(path)?;
        Ok(Self::new(&data))
    }
}

#[derive(Debug)]
pub struct RomHeader {
    pub entrypoint: Disassembly,
    pub valid_nintendo_logo: bool,
}

impl RomHeader {
    pub fn new(data: &[u8]) -> Self {
        let mut entrypoint = Disassembly::new();
        entrypoint.decode_range(&data, 0x100, 0x104);

        let valid_nintendo_logo = data[0x104..=0x133] == NINTENDO_LOGO;

        Self {
            entrypoint,
            valid_nintendo_logo,
        }
    }
}

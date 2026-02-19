use crate::disassembler::Disassembly;
use crate::rom::header::RomHeader;
use std::path::Path;

mod header;

#[derive(Debug)]
pub struct Rom {
    pub data: Vec<u8>,
}

impl Rom {
    pub fn new(data: &[u8]) -> Self {
        Self {
            data: data.to_vec(),
        }
    }

    pub fn from_file(path: &Path) -> Result<Self, std::io::Error> {
        let data = std::fs::read(path)?;
        Ok(Self::new(&data))
    }
}

// Header
impl Rom {
    pub fn header(&self) -> RomHeader {
        RomHeader::new(&self.data)
    }

    pub fn title(&self) -> String {
        RomHeader::parse_title(&self.data)
    }

    pub fn has_valid_nintendo_logo(&self) -> bool {
        RomHeader::parse_valid_nintendo_logo(&self.data)
    }

    pub fn cgb_mode(&self) -> header::RomCgbMode {
        RomHeader::parse_cgb_mode(&self.data)
    }

    pub fn sgb_support(&self) -> bool {
        RomHeader::parse_sgb_support(&self.data)
    }

    pub fn licensee(&self) -> header::RomLicensee {
        RomHeader::parse_licensee(&self.data)
    }

    pub fn cartridge_type(&self) -> Option<header::RomCartridgeType> {
        RomHeader::parse_cartridge_type(&self.data)
    }

    pub fn rom_banks(&self) -> usize {
        RomHeader::parse_rom_banks(&self.data)
    }

    pub fn ram_banks(&self) -> usize {
        RomHeader::parse_ram_banks(&self.data)
    }

    pub fn overseas_only(&self) -> bool {
        RomHeader::parse_overseas_only(&self.data)
    }

    pub fn version_number(&self) -> u8 {
        RomHeader::parse_version_number(&self.data)
    }

    pub fn provided_header_checksum(&self) -> u8 {
        RomHeader::parse_header_checksum(&self.data)
    }

    pub fn actual_header_checksum(&self) -> u8 {
        RomHeader::calculate_header_checksum(&self.data)
    }

    pub fn provided_global_checksum(&self) -> u16 {
        RomHeader::parse_global_checksum(&self.data)
    }

    pub fn actual_global_checksum(&self) -> u16 {
        RomHeader::calculate_global_checksum(&self.data)
    }

    pub fn entrypoint(&self) -> Disassembly {
        RomHeader::parse_entrypoint(&self.data)
    }
}

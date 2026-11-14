use crate::error::GbResult;
use crate::rom::header::RomHeader;
use std::path::Path;
use std::sync::Arc;

pub mod header;
#[cfg(feature = "rom-id")]
pub mod id;
pub mod nds;

#[derive(Debug, Clone)]
pub struct Rom {
    pub data: Arc<[u8]>,
    #[cfg(feature = "rom-id")]
    ids: std::sync::OnceLock<(id::RomId, [u8; 20])>,
}

impl Rom {
    pub fn new(data: &[u8]) -> Self {
        Self::from(Arc::<[u8]>::from(data))
    }

    #[cfg(feature = "rom-id")]
    pub fn with_ids(self, ids: (id::RomId, [u8; 20])) -> Self {
        Self {
            ids: ids.into(),
            ..self
        }
    }

    #[cfg(feature = "rom-id")]
    pub fn ids(&self) -> (id::RomId, [u8; 20]) {
        *self
            .ids
            .get_or_init(|| id::RomId::with_game_sha1(&self.data))
    }

    pub fn from_file(path: &Path) -> GbResult<Self> {
        Ok(Self::from(std::fs::read(path)?))
    }
}

impl From<Arc<[u8]>> for Rom {
    fn from(data: Arc<[u8]>) -> Self {
        Self {
            data,
            #[cfg(feature = "rom-id")]
            ids: std::sync::OnceLock::new(),
        }
    }
}

impl From<Vec<u8>> for Rom {
    fn from(data: Vec<u8>) -> Self {
        Self::from(Arc::<[u8]>::from(data))
    }
}

// Header
impl Rom {
    pub fn header(&self) -> GbResult<RomHeader> {
        RomHeader::new(&self.data)
    }

    pub fn title(&self) -> GbResult<String> {
        RomHeader::parse_title(&self.data)
    }

    pub fn has_valid_nintendo_logo(&self) -> GbResult<bool> {
        RomHeader::parse_valid_nintendo_logo(&self.data)
    }

    pub fn cgb_mode(&self) -> GbResult<header::RomCgbMode> {
        RomHeader::parse_cgb_mode(&self.data)
    }

    pub fn sgb_support(&self) -> GbResult<bool> {
        RomHeader::parse_sgb_support(&self.data)
    }

    pub fn licensee(&self) -> GbResult<header::RomLicensee> {
        RomHeader::parse_licensee(&self.data)
    }

    pub fn cartridge_type(&self) -> GbResult<Option<header::RomCartridgeType>> {
        RomHeader::parse_cartridge_type(&self.data)
    }

    pub fn rom_banks(&self) -> GbResult<usize> {
        RomHeader::parse_rom_banks(&self.data)
    }

    pub fn ram_banks(&self) -> GbResult<usize> {
        RomHeader::parse_ram_banks(&self.data)
    }

    pub fn overseas_only(&self) -> GbResult<bool> {
        RomHeader::parse_overseas_only(&self.data)
    }

    pub fn version_number(&self) -> GbResult<u8> {
        RomHeader::parse_version_number(&self.data)
    }

    pub fn provided_header_checksum(&self) -> GbResult<u8> {
        RomHeader::parse_header_checksum(&self.data)
    }

    pub fn actual_header_checksum(&self) -> GbResult<u8> {
        RomHeader::calculate_header_checksum(&self.data)
    }

    pub fn provided_global_checksum(&self) -> GbResult<u16> {
        RomHeader::parse_global_checksum(&self.data)
    }

    pub fn actual_global_checksum(&self) -> GbResult<u16> {
        RomHeader::calculate_global_checksum(&self.data)
    }
}

#[cfg(all(test, feature = "rom-id"))]
mod tests {
    use super::*;

    #[test]
    fn a_rom_hashes_once_and_its_clones_keep_the_ids() {
        let data = vec![7u8; 0x4000];
        let ids = id::RomId::with_game_sha1(&data);
        let rom = Rom::from(data.clone());
        assert!(rom.ids.get().is_none());
        assert_eq!(rom.ids(), ids);
        assert_eq!(rom.clone().ids.get(), Some(&ids));
        let given = (id::RomId::default(), [1; 20]);
        assert_eq!(Rom::from(data).with_ids(given).ids(), given);
    }
}

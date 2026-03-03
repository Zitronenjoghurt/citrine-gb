use crate::error::GbResult;
use crate::gb::cartridge::RAM_BANK_SIZE;
use std::path::Path;

/// A simple SRAM dump
pub struct SDump(Vec<u8>);

impl SDump {
    pub fn from_slice(data: &[u8]) -> Self {
        Self(data.to_vec())
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    pub fn from_banks(banks: &[[u8; RAM_BANK_SIZE]]) -> Self {
        let mut data = Vec::new();
        for bank in banks {
            data.extend_from_slice(bank);
        }
        Self(data)
    }

    pub fn save(&self, path: &Path) -> GbResult<()> {
        std::fs::write(path, &self.0)?;
        Ok(())
    }

    pub fn load(path: &Path) -> GbResult<Self> {
        let data = std::fs::read(path)?;
        Ok(Self(data))
    }

    #[cfg(feature = "base64")]
    pub fn to_base64(&self) -> GbResult<String> {
        use base64::Engine;
        Ok(base64::engine::general_purpose::STANDARD.encode(&self.0))
    }

    #[cfg(feature = "base64")]
    pub fn from_base64(string: &str) -> GbResult<Self> {
        use base64::Engine;
        Ok(base64::engine::general_purpose::STANDARD
            .decode(string)
            .map(Self)?)
    }
}

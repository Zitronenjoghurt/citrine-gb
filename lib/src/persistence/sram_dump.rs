use crate::error::GbResult;
use crate::gb::cartridge::RAM_BANK_SIZE;
use std::io::Write;
use std::path::Path;

/// A simple SRAM dump
#[derive(serde::Serialize, serde::Deserialize)]
pub struct SramDump(Vec<u8>);

impl SramDump {
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

    #[cfg(feature = "brotli")]
    pub fn to_brotli(&self) -> GbResult<Vec<u8>> {
        let mut compressed_data = Vec::new();
        {
            let mut writer = brotli::CompressorWriter::new(&mut compressed_data, 4096, 11, 22);
            writer.write_all(&self.0)?;
        }
        Ok(compressed_data)
    }

    #[cfg(feature = "brotli")]
    pub fn from_brotli(data: &[u8]) -> GbResult<Self> {
        let mut decompressed = Vec::new();
        let mut decompressor = brotli::Decompressor::new(data, 4096);
        std::io::Read::read_to_end(&mut decompressor, &mut decompressed)?;
        Ok(Self(decompressed))
    }

    #[cfg(feature = "brotli")]
    pub fn save_compressed(&self, path: &Path) -> GbResult<()> {
        let compressed_data = self.to_brotli()?;
        std::fs::write(path, &compressed_data)?;
        Ok(())
    }

    #[cfg(feature = "brotli")]
    pub fn load_compressed(path: &Path) -> GbResult<Self> {
        let compressed_data = std::fs::read(path)?;
        Self::from_brotli(&compressed_data)
    }

    #[cfg(all(feature = "base64", feature = "brotli"))]
    pub fn to_compressed_base64(&self) -> GbResult<String> {
        use base64::Engine;
        let compressed_data = self.to_brotli()?;
        Ok(base64::engine::general_purpose::STANDARD.encode(&compressed_data))
    }

    #[cfg(all(feature = "base64", feature = "brotli"))]
    pub fn from_compressed_base64(string: &str) -> GbResult<Self> {
        use base64::Engine;
        Self::from_brotli(&base64::engine::general_purpose::STANDARD.decode(string)?)
    }
}

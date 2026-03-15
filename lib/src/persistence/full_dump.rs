use crate::error::GbResult;
use crate::gb::GameBoy;
use crate::persistence::sram_dump::SramDump;
use std::io::Write;

#[derive(serde::Serialize)]
pub struct FullDumpRef<'a> {
    pub gb: &'a GameBoy,
    pub sram: Option<SramDump>,
}

impl<'a> FullDumpRef<'a> {
    pub fn to_rmp(&self) -> GbResult<Vec<u8>> {
        Ok(rmp_serde::to_vec_named(self)?)
    }

    #[cfg(feature = "brotli")]
    pub fn to_brotli(&self) -> GbResult<Vec<u8>> {
        let mut compressed_data = Vec::new();
        {
            let mut writer = brotli::CompressorWriter::new(&mut compressed_data, 4096, 11, 22);
            writer.write_all(&self.to_rmp()?)?;
        }
        Ok(compressed_data)
    }

    #[cfg(feature = "serde_json")]
    pub fn to_json(&self) -> GbResult<String> {
        Ok(serde_json::to_string(self)?)
    }

    #[cfg(feature = "serde_json")]
    pub fn to_json_pretty(&self) -> GbResult<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct FullDump {
    pub gb: GameBoy,
    pub sram: Option<SramDump>,
}

impl FullDump {
    pub fn from_rmp(data: &[u8]) -> GbResult<Self> {
        Ok(rmp_serde::from_slice(data)?)
    }

    #[cfg(feature = "brotli")]
    pub fn from_brotli(data: &[u8]) -> GbResult<Self> {
        let mut decompressed = Vec::new();
        let mut decompressor = brotli::Decompressor::new(data, 4096);
        std::io::Read::read_to_end(&mut decompressor, &mut decompressed)?;
        Self::from_rmp(&decompressed)
    }

    #[cfg(feature = "serde_json")]
    pub fn from_json(data: &str) -> GbResult<Self> {
        Ok(serde_json::from_str(data)?)
    }
}

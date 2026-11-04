use crate::console::ConsoleKind;
use crate::error::GbResult;
use crate::gb::GbModel;
#[cfg(feature = "brotli")]
use std::io::Write;

pub const SAVE_QUALITY: u32 = 11;
pub const GBA_SAVE_QUALITY: u32 = 1;
pub const GBA_STATE_MAGIC: [u8; 4] = *b"CGBA";
pub const GBA_STATE_VERSION: u8 = 1;

pub struct StateBody {
    pub kind: ConsoleKind,
    pub data: Vec<u8>,
}

impl StateBody {
    pub fn pack(&self, quality: u32) -> GbResult<Vec<u8>> {
        let body = compress(&self.data, quality)?;
        Ok(match self.kind {
            ConsoleKind::Gb | ConsoleKind::Gbc => body,
            ConsoleKind::Gba => [&GBA_STATE_MAGIC[..], &[GBA_STATE_VERSION], &body].concat(),
        })
    }
}

#[cfg(feature = "brotli")]
fn compress(data: &[u8], quality: u32) -> GbResult<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut writer = brotli::CompressorWriter::new(&mut out, 4096, quality, 22);
        writer.write_all(data)?;
    }
    Ok(out)
}

#[cfg(not(feature = "brotli"))]
fn compress(data: &[u8], _quality: u32) -> GbResult<Vec<u8>> {
    Ok(data.to_vec())
}

#[cfg(feature = "brotli")]
fn decompress(data: &[u8]) -> GbResult<Vec<u8>> {
    let mut out = Vec::new();
    std::io::Read::read_to_end(&mut brotli::Decompressor::new(data, 4096), &mut out)?;
    Ok(out)
}

#[cfg(not(feature = "brotli"))]
fn decompress(data: &[u8]) -> GbResult<Vec<u8>> {
    Ok(data.to_vec())
}

#[derive(serde::Deserialize)]
struct GbStateKind {
    gb: GbModelOnly,
}

#[derive(serde::Deserialize)]
struct GbModelOnly {
    model: GbModel,
}

pub fn is_gba_state(state: &[u8]) -> bool {
    state.starts_with(&GBA_STATE_MAGIC)
}

#[cfg(feature = "gba")]
pub fn gba_state_body(state: &[u8]) -> GbResult<Vec<u8>> {
    match state.strip_prefix(&GBA_STATE_MAGIC[..]) {
        Some([GBA_STATE_VERSION, body @ ..]) => decompress(body),
        Some(_) => Err(crate::error::GbError::NewerState),
        None => Err(crate::error::GbError::NotGba),
    }
}

pub fn state_kind(state: &[u8]) -> Option<ConsoleKind> {
    if is_gba_state(state) {
        return Some(ConsoleKind::Gba);
    }
    let body = decompress(state).ok()?;
    let peek: GbStateKind = rmp_serde::from_slice(&body).ok()?;
    Some(peek.gb.model.into())
}

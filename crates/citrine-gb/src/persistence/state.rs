use crate::console::ConsoleKind;
use crate::error::GbResult;
use crate::gb::GbModel;
#[cfg(feature = "brotli")]
use std::io::Write;

pub const SAVE_QUALITY: u32 = 11;
pub const GBA_SAVE_QUALITY: u32 = 1;
pub const GBA_WORKER_QUALITY: u32 = 5;
pub const BIG_STATE_BYTES: usize = 2 * 1024 * 1024;
pub const BIG_STATE_QUALITY: u32 = 1;
pub const GBA_STATE_MAGIC: [u8; 4] = *b"CGBA";
pub const GBA_STATE_VERSION: u8 = 1;
pub const NDS_STATE_MAGIC: [u8; 4] = *b"CNDS";
pub const NDS_STATE_VERSION: u8 = 1;

pub struct StateBody {
    pub kind: ConsoleKind,
    pub data: Vec<u8>,
}

impl StateBody {
    pub fn quality(&self, off_ui: bool) -> u32 {
        match self.kind {
            ConsoleKind::Gb | ConsoleKind::Gbc => SAVE_QUALITY,
            ConsoleKind::Nds => BIG_STATE_QUALITY,
            _ if self.data.len() > BIG_STATE_BYTES => BIG_STATE_QUALITY,
            ConsoleKind::Gba if off_ui => GBA_WORKER_QUALITY,
            ConsoleKind::Gba => GBA_SAVE_QUALITY,
        }
    }

    pub fn pack(&self, quality: u32) -> GbResult<Vec<u8>> {
        let body = compress(&self.data, quality)?;
        Ok(match self.kind {
            ConsoleKind::Gb | ConsoleKind::Gbc => body,
            ConsoleKind::Gba => [&GBA_STATE_MAGIC[..], &[GBA_STATE_VERSION], &body].concat(),
            ConsoleKind::Nds => [&NDS_STATE_MAGIC[..], &[NDS_STATE_VERSION], &body].concat(),
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
        Some(rest) => versioned_body(rest, GBA_STATE_VERSION),
        None => Err(crate::error::GbError::NotGba),
    }
}

pub fn is_nds_state(state: &[u8]) -> bool {
    state.starts_with(&NDS_STATE_MAGIC)
}

#[cfg(feature = "nds")]
pub fn nds_state_body(state: &[u8]) -> GbResult<Vec<u8>> {
    match state.strip_prefix(&NDS_STATE_MAGIC[..]) {
        Some(rest) => versioned_body(rest, NDS_STATE_VERSION),
        None => Err(crate::error::GbError::NotNds),
    }
}

#[cfg(feature = "gba")]
fn versioned_body(rest: &[u8], current: u8) -> GbResult<Vec<u8>> {
    use crate::error::GbError;
    match rest {
        [version, body @ ..] if *version == current => decompress(body),
        [version, ..] if *version < current => Err(GbError::OlderState),
        [_, ..] => Err(GbError::NewerState),
        [] => Err(GbError::IO(std::io::ErrorKind::UnexpectedEof.into())),
    }
}

pub fn state_kind(state: &[u8]) -> Option<ConsoleKind> {
    if is_gba_state(state) {
        return Some(ConsoleKind::Gba);
    }
    if is_nds_state(state) {
        return Some(ConsoleKind::Nds);
    }
    let body = decompress(state).ok()?;
    let peek: GbStateKind = rmp_serde::from_slice(&body).ok()?;
    Some(peek.gb.model.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ReadMemory;
    use crate::WriteMemory;
    use crate::gb::{GameBoy, GbModel};
    use crate::rom::Rom;

    fn body(kind: ConsoleKind, len: usize) -> StateBody {
        StateBody {
            kind,
            data: vec![0; len],
        }
    }

    #[test]
    fn the_level_follows_console_size_and_thread() {
        assert_eq!(body(ConsoleKind::Gb, 40_000).quality(true), SAVE_QUALITY);
        assert_eq!(body(ConsoleKind::Gbc, 40_000).quality(false), SAVE_QUALITY);
        assert_eq!(
            body(ConsoleKind::Gba, 600_000).quality(true),
            GBA_WORKER_QUALITY
        );
        assert_eq!(
            body(ConsoleKind::Gba, 600_000).quality(false),
            GBA_SAVE_QUALITY
        );
        let big = body(ConsoleKind::Gba, BIG_STATE_BYTES + 1);
        assert_eq!(big.quality(true), BIG_STATE_QUALITY);
        assert_eq!(big.quality(false), BIG_STATE_QUALITY);
    }

    fn test_rom() -> Vec<u8> {
        let mut data = vec![0u8; 0x8000];
        data[0x0147] = 0x03;
        data[0x0148] = 0x00;
        data[0x0149] = 0x02;
        data[0x0100] = 0xAB;
        data[0x3FFF] = 0xCD;
        data
    }

    fn loaded() -> GameBoy {
        let data = test_rom();
        let rom = Rom::new(&data);
        let mut gb = GameBoy::new_empty(GbModel::Dmg);
        gb.load_rom(&rom).expect("load");
        gb
    }

    #[test]
    fn restored_snapshot_can_read_rom() {
        let mut gb = loaded();
        for _ in 0..1000 {
            gb.step();
        }

        let dump = gb.dump_full(0).expect("dump");
        let rom_data = test_rom();
        let restored = GameBoy::from_dump(&dump, &Rom::new(&rom_data), 0).expect("restore");

        assert_eq!(restored.cartridge.read_naive(0x0100), 0xAB);
        assert_eq!(restored.cartridge.read_naive(0x3FFF), 0xCD);
    }

    #[test]
    fn restored_snapshot_keeps_cpu_state() {
        let mut gb = loaded();
        for _ in 0..1000 {
            gb.step();
        }
        let (pc, sp, a) = (gb.cpu.pc, gb.cpu.sp, gb.cpu.a);

        let dump = gb.dump_full(0).expect("dump");
        let rom_data = test_rom();
        let restored = GameBoy::from_dump(&dump, &Rom::new(&rom_data), 0).expect("restore");

        assert_eq!(restored.cpu.pc, pc);
        assert_eq!(restored.cpu.sp, sp);
        assert_eq!(restored.cpu.a, a);
    }

    #[test]
    fn restored_snapshot_keeps_cartridge_ram() {
        let mut gb = loaded();
        gb.cartridge.write_naive(0x0000, 0x0A);
        for (i, addr) in (0xA000..0xA010u16).enumerate() {
            gb.cartridge.write_naive(addr, 0x40 + i as u8);
        }

        let dump = gb.dump_full(0).expect("dump");
        let rom_data = test_rom();
        let mut restored = GameBoy::from_dump(&dump, &Rom::new(&rom_data), 0).expect("restore");

        restored.cartridge.write_naive(0x0000, 0x0A);
        for (i, addr) in (0xA000..0xA010u16).enumerate() {
            assert_eq!(
                restored.cartridge.read_naive(addr),
                0x40 + i as u8,
                "SRAM byte {i} did not survive the round trip"
            );
        }
    }

    #[test]
    fn a_restored_machine_keeps_running() {
        let mut gb = loaded();
        for _ in 0..1000 {
            gb.step();
        }
        let dump = gb.dump_full(0).expect("dump");
        let rom_data = test_rom();
        let mut restored = GameBoy::from_dump(&dump, &Rom::new(&rom_data), 0).expect("restore");

        for _ in 0..1000 {
            restored.step();
        }
    }
}

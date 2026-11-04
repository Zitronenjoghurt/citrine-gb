use super::{GameBoy, GbModel};
use crate::console::{Console, ConsoleKind, Keys, Timing, VideoSpec};
use crate::error::GbError;
#[cfg(feature = "persistence")]
use crate::error::GbResult;
#[cfg(feature = "persistence")]
use crate::persistence::battery_save::BatterySave;
#[cfg(feature = "persistence")]
use crate::persistence::state::StateBody;
#[cfg(feature = "rom-id")]
use crate::rom::id::RomId;

impl From<GbModel> for ConsoleKind {
    fn from(model: GbModel) -> Self {
        match model {
            GbModel::Dmg => ConsoleKind::Gb,
            GbModel::Cgb => ConsoleKind::Gbc,
        }
    }
}

impl TryFrom<ConsoleKind> for GbModel {
    type Error = GbError;

    fn try_from(kind: ConsoleKind) -> Result<Self, GbError> {
        match kind {
            ConsoleKind::Gb => Ok(GbModel::Dmg),
            ConsoleKind::Gbc => Ok(GbModel::Cgb),
            ConsoleKind::Gba => Err(GbError::GbaGame),
        }
    }
}

impl Console for GameBoy {
    fn kind(&self) -> ConsoleKind {
        self.model.into()
    }

    fn video(&self) -> VideoSpec {
        self.kind().video()
    }

    fn timing(&self) -> Timing {
        self.kind().timing()
    }

    fn run_frame(&mut self) {
        GameBoy::run_frame(self);
    }

    fn frame(&self) -> &[u8] {
        GameBoy::frame(self).as_slice()
    }

    fn take_audio(&mut self, out: &mut Vec<f32>) {
        out.append(&mut self.apu.audio_buffer);
    }

    fn sample_rate(&self) -> u32 {
        self.apu.output_sample_rate
    }

    fn set_sample_rate(&mut self, rate: u32) {
        self.apu.set_sample_rate(rate);
    }

    fn keys(&self) -> Keys {
        GameBoy::keys(self)
    }

    fn set_keys(&mut self, keys: Keys) {
        GameBoy::set_keys(self, keys);
    }

    fn soft_reset(&mut self) {
        GameBoy::soft_reset(self);
    }

    fn audio_channels(&self) -> &'static [&'static str] {
        GameBoy::audio_channels(self)
    }

    fn set_channel_enabled(&mut self, index: usize, enabled: bool) {
        self.apu.set_channel_enabled(index, enabled);
    }

    fn rom_bytes(&self) -> Vec<u8> {
        self.cartridge.rom_bytes()
    }

    fn title(&self) -> String {
        self.cartridge.header.short_title().to_string()
    }

    fn has_battery(&self) -> bool {
        self.cartridge.has_battery()
    }

    fn has_clock(&self) -> bool {
        self.cartridge.rtc().is_some()
    }

    #[cfg(feature = "persistence")]
    fn battery(&mut self, force: bool, now: u64) -> Option<BatterySave> {
        self.cartridge.poll_battery(force, now)
    }

    #[cfg(feature = "persistence")]
    fn battery_from_file(&self, data: &[u8]) -> BatterySave {
        self.cartridge.read_battery(data)
    }

    #[cfg(feature = "persistence")]
    fn put_battery(&mut self, save: BatterySave, now: u64) {
        self.cartridge.put_battery(save, now);
    }

    #[cfg(feature = "persistence")]
    fn save_state(&mut self, now: u64) -> GbResult<Vec<u8>> {
        self.dump_full(now)
    }

    #[cfg(feature = "persistence")]
    fn state_body(&mut self, now: u64) -> GbResult<StateBody> {
        let sram = self.poll_sram_dump(true, now);
        let data = crate::persistence::full_dump::FullDumpRef { gb: self, sram }.to_rmp()?;
        Ok(StateBody {
            kind: self.kind(),
            data,
        })
    }

    #[cfg(feature = "rom-id")]
    fn rom_id(&self) -> RomId {
        self.cartridge.header.id()
    }
}

#[cfg(feature = "debug")]
mod debug {
    use super::GameBoy;
    use crate::debug::breakpoints::Breakpoints;
    use crate::debug::code::{Address, Line};
    use crate::debug::memory::MemoryBlock;
    use crate::debug::registers::RegisterGroup;
    use crate::debug::step::{Step, Stop};
    use crate::debug::trace::TraceLog;
    use crate::debug::video::{Palette, SheetColors, Sprite, TileMap, TileSheet};
    use crate::debug::{Debug, Features};
    use std::collections::VecDeque;
    use std::ops::Range;

    const FEATURES: Features = Features {
        pc_digits: 4,
        thumb: false,
        vectors: &[
            ("VBlank", 0x40),
            ("STAT", 0x48),
            ("Timer", 0x50),
            ("Serial", 0x58),
            ("Joypad", 0x60),
        ],
        cpu_groups: &["CPU"],
        watchpoints: true,
        cycle: "M-cycle",
    };

    impl Debug for GameBoy {
        fn features(&self) -> Features {
            FEATURES
        }

        fn registers(&self, out: &mut Vec<RegisterGroup>) {
            GameBoy::registers(self, out);
        }

        fn memory_blocks(&self, out: &mut Vec<MemoryBlock>) {
            GameBoy::memory_blocks(self, out);
        }

        fn locate(&self, addr: u32) -> Option<Address> {
            u16::try_from(addr)
                .ok()
                .and_then(|addr| self.address_of(addr))
        }

        fn peek(&self, at: Address, out: &mut [u8]) {
            GameBoy::peek(self, at, out);
        }

        fn poke(&mut self, at: Address, bytes: &[u8]) {
            GameBoy::poke(self, at, bytes);
        }

        fn pc(&self) -> Option<Address> {
            self.pc_address()
        }

        fn code_analysis(&self) -> bool {
            self.debugger.static_analysis_enabled
        }

        fn set_code_analysis(&mut self, on: bool) {
            self.debugger.static_analysis_enabled = on;
        }

        fn code_rows(&self) -> usize {
            GameBoy::code_rows(self)
        }

        fn code_row_of(&self, at: Address) -> Option<usize> {
            GameBoy::code_row_of(self, at)
        }

        fn disassemble(&self, rows: Range<usize>, out: &mut Vec<Line>) {
            GameBoy::disassemble(self, rows, out);
        }

        fn breakpoints(&mut self) -> &mut Breakpoints {
            &mut self.debugger.breakpoints
        }

        fn cycles(&self) -> u128 {
            self.debugger.total_cycles
        }

        fn record_waves(&mut self, on: bool) {
            self.debugger.record_channels = on;
        }

        fn waves(&self) -> &[VecDeque<f32>] {
            self.debugger.waves()
        }

        fn step(&mut self, step: Step) -> Stop {
            self.run_step(step)
        }

        fn step_pending(&self) -> bool {
            GameBoy::step_pending(self)
        }

        fn resume_step(&mut self) -> Stop {
            GameBoy::resume_step(self)
        }

        fn cancel_step(&mut self) {
            GameBoy::cancel_step(self);
        }

        fn take_breakpoint_hit(&mut self) -> bool {
            std::mem::take(&mut self.debugger.hit_breakpoint)
        }

        fn frame_cycles(&self) -> u32 {
            GameBoy::frame_cycles(self)
        }

        fn scanline(&self) -> u16 {
            u16::from(self.ppu.line())
        }

        fn trace(&mut self) -> &mut TraceLog {
            &mut self.debugger.trace
        }

        fn tile_sheets(&self, colors: SheetColors, out: &mut Vec<TileSheet>) {
            GameBoy::tile_sheets(self, colors, out);
        }

        fn tile_maps(&self, out: &mut Vec<TileMap>) {
            GameBoy::tile_maps(self, out);
        }

        fn sprites(&self, out: &mut Vec<Sprite>) {
            GameBoy::sprites(self, out);
        }

        fn palettes(&self, out: &mut Vec<Palette>) {
            GameBoy::palettes(self, out);
        }

        fn print_log(&mut self) -> Option<&mut crate::debug::print::PrintLog> {
            None
        }
    }
}

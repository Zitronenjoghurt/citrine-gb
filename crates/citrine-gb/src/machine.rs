use crate::console::{Console, ConsoleKind, Keys, Timing, VideoSpec};
use crate::error::GbResult;
use crate::gb::{GameBoy, GbModel};
#[cfg(feature = "gba")]
use crate::gba::bios::{Bios, Boot};
#[cfg(feature = "gba")]
use crate::gba::machine::Gba;
#[cfg(feature = "persistence")]
use crate::persistence::battery_save::BatterySave;
#[cfg(feature = "persistence")]
use crate::persistence::state::StateBody;
use crate::rom::Rom;
#[cfg(feature = "rom-id")]
use crate::rom::id::RomId;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Firmware {
    pub gb_boot_rom: Option<Vec<u8>>,
    #[cfg(feature = "gba")]
    pub gba_bios: Option<Bios>,
    #[cfg(feature = "gba")]
    pub gba_boot: Boot,
}

pub enum Machine {
    Gb(Box<GameBoy>),
    #[cfg(feature = "gba")]
    Gba(Box<Gba>),
}

impl Machine {
    pub fn load(rom: &Rom, kind: ConsoleKind, firmware: &Firmware) -> GbResult<Self> {
        #[cfg(feature = "gba")]
        if kind == ConsoleKind::Gba {
            let bios = firmware.gba_bios.clone().unwrap_or_default();
            let gba = Gba::with_bios(&rom.data, bios, firmware.gba_boot);
            return Ok(Self::Gba(Box::new(gba)));
        }
        let model = GbModel::try_from(kind)?;
        let mut gb = GameBoy::new_empty(model);
        if let Some(boot_rom) = &firmware.gb_boot_rom {
            gb.load_boot_rom(boot_rom);
        }
        gb.load_rom_with_model(rom, model)?;
        Ok(Self::Gb(Box::new(gb)))
    }

    #[cfg(feature = "persistence")]
    pub fn from_state(
        state: &[u8],
        rom: &Rom,
        now: u64,
        #[cfg_attr(not(feature = "gba"), expect(unused_variables))] firmware: &Firmware,
    ) -> GbResult<Self> {
        if crate::persistence::state::is_gba_state(state) {
            #[cfg(feature = "gba")]
            return Gba::with_bios(
                &rom.data,
                firmware.gba_bios.clone().unwrap_or_default(),
                firmware.gba_boot,
            )
            .restore(state)
            .map(|gba| Self::Gba(Box::new(gba)));
            #[cfg(not(feature = "gba"))]
            return Err(crate::error::GbError::GbaGame);
        }
        GameBoy::from_dump(state, rom, now).map(|gb| Self::Gb(Box::new(gb)))
    }

    #[cfg(feature = "persistence")]
    pub fn restore(&self, state: &[u8], now: u64, firmware: &Firmware) -> GbResult<Self> {
        #[cfg(feature = "gba")]
        if let Self::Gba(gba) = self
            && crate::persistence::state::is_gba_state(state)
        {
            return gba.restore(state).map(|gba| Self::Gba(Box::new(gba)));
        }
        Self::from_state(state, &Rom::new(&self.rom_bytes()), now, firmware)
    }

    pub fn as_gb(&self) -> Option<&GameBoy> {
        match self {
            Self::Gb(gb) => Some(gb),
            #[cfg(feature = "gba")]
            Self::Gba(_) => None,
        }
    }

    pub fn as_gb_mut(&mut self) -> Option<&mut GameBoy> {
        match self {
            Self::Gb(gb) => Some(gb),
            #[cfg(feature = "gba")]
            Self::Gba(_) => None,
        }
    }

    fn console(&self) -> &dyn Console {
        match self {
            Self::Gb(gb) => gb.as_ref(),
            #[cfg(feature = "gba")]
            Self::Gba(gba) => gba.as_ref(),
        }
    }

    fn console_mut(&mut self) -> &mut dyn Console {
        match self {
            Self::Gb(gb) => gb.as_mut(),
            #[cfg(feature = "gba")]
            Self::Gba(gba) => gba.as_mut(),
        }
    }
}

impl Console for Machine {
    fn kind(&self) -> ConsoleKind {
        self.console().kind()
    }

    fn video(&self) -> VideoSpec {
        self.console().video()
    }

    fn timing(&self) -> Timing {
        self.console().timing()
    }

    fn run_frame(&mut self) {
        self.console_mut().run_frame();
    }

    fn frame(&self) -> &[u8] {
        self.console().frame()
    }

    fn take_audio(&mut self, out: &mut Vec<f32>) {
        self.console_mut().take_audio(out);
    }

    fn sample_rate(&self) -> u32 {
        self.console().sample_rate()
    }

    fn set_sample_rate(&mut self, rate: u32) {
        self.console_mut().set_sample_rate(rate);
    }

    fn keys(&self) -> Keys {
        self.console().keys()
    }

    fn set_keys(&mut self, keys: Keys) {
        self.console_mut().set_keys(keys);
    }

    fn soft_reset(&mut self) {
        self.console_mut().soft_reset();
    }

    fn audio_channels(&self) -> &'static [&'static str] {
        self.console().audio_channels()
    }

    fn set_channel_enabled(&mut self, index: usize, enabled: bool) {
        self.console_mut().set_channel_enabled(index, enabled);
    }

    fn rom_bytes(&self) -> Vec<u8> {
        self.console().rom_bytes()
    }

    fn title(&self) -> String {
        self.console().title()
    }

    fn has_battery(&self) -> bool {
        self.console().has_battery()
    }

    fn has_clock(&self) -> bool {
        self.console().has_clock()
    }

    #[cfg(feature = "persistence")]
    fn battery(&mut self, force: bool, now: u64) -> Option<BatterySave> {
        self.console_mut().battery(force, now)
    }

    #[cfg(feature = "persistence")]
    fn battery_from_file(&self, data: &[u8]) -> BatterySave {
        self.console().battery_from_file(data)
    }

    #[cfg(feature = "persistence")]
    fn put_battery(&mut self, save: BatterySave, now: u64) {
        self.console_mut().put_battery(save, now);
    }

    #[cfg(feature = "persistence")]
    fn save_state(&mut self, now: u64) -> GbResult<Vec<u8>> {
        self.console_mut().save_state(now)
    }

    #[cfg(feature = "persistence")]
    fn state_body(&mut self, now: u64) -> GbResult<StateBody> {
        self.console_mut().state_body(now)
    }

    #[cfg(feature = "rom-id")]
    fn rom_id(&self) -> RomId {
        self.console().rom_id()
    }
}

#[cfg(feature = "debug")]
mod debug {
    use super::Machine;
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

    impl Machine {
        fn debug(&self) -> &dyn Debug {
            match self {
                Self::Gb(gb) => gb.as_ref(),
                #[cfg(feature = "gba")]
                Self::Gba(gba) => gba.as_ref(),
            }
        }

        fn debug_mut(&mut self) -> &mut dyn Debug {
            match self {
                Self::Gb(gb) => gb.as_mut(),
                #[cfg(feature = "gba")]
                Self::Gba(gba) => gba.as_mut(),
            }
        }
    }

    impl Debug for Machine {
        fn features(&self) -> Features {
            self.debug().features()
        }

        fn registers(&self, out: &mut Vec<RegisterGroup>) {
            self.debug().registers(out);
        }

        fn memory_blocks(&self, out: &mut Vec<MemoryBlock>) {
            self.debug().memory_blocks(out);
        }

        fn locate(&self, addr: u32) -> Option<Address> {
            self.debug().locate(addr)
        }

        fn peek(&self, at: Address, out: &mut [u8]) {
            self.debug().peek(at, out);
        }

        fn poke(&mut self, at: Address, bytes: &[u8]) {
            self.debug_mut().poke(at, bytes);
        }

        fn pc(&self) -> Option<Address> {
            self.debug().pc()
        }

        fn code_analysis(&self) -> bool {
            self.debug().code_analysis()
        }

        fn set_code_analysis(&mut self, on: bool) {
            self.debug_mut().set_code_analysis(on);
        }

        fn code_rows(&self) -> usize {
            self.debug().code_rows()
        }

        fn code_row_of(&self, at: Address) -> Option<usize> {
            self.debug().code_row_of(at)
        }

        fn disassemble(&self, rows: Range<usize>, out: &mut Vec<Line>) {
            self.debug().disassemble(rows, out);
        }

        fn breakpoints(&mut self) -> &mut Breakpoints {
            self.debug_mut().breakpoints()
        }

        fn cycles(&self) -> u128 {
            self.debug().cycles()
        }

        fn record_waves(&mut self, on: bool) {
            self.debug_mut().record_waves(on);
        }

        fn waves(&self) -> &[VecDeque<f32>] {
            self.debug().waves()
        }

        fn step(&mut self, step: Step) -> Stop {
            self.debug_mut().step(step)
        }

        fn step_pending(&self) -> bool {
            self.debug().step_pending()
        }

        fn resume_step(&mut self) -> Stop {
            self.debug_mut().resume_step()
        }

        fn cancel_step(&mut self) {
            self.debug_mut().cancel_step();
        }

        fn take_breakpoint_hit(&mut self) -> bool {
            self.debug_mut().take_breakpoint_hit()
        }

        fn frame_cycles(&self) -> u32 {
            self.debug().frame_cycles()
        }

        fn scanline(&self) -> u16 {
            self.debug().scanline()
        }

        fn trace(&mut self) -> &mut TraceLog {
            self.debug_mut().trace()
        }

        fn tile_sheets(&self, colors: SheetColors, out: &mut Vec<TileSheet>) {
            self.debug().tile_sheets(colors, out);
        }

        fn tile_maps(&self, out: &mut Vec<TileMap>) {
            self.debug().tile_maps(out);
        }

        fn sprites(&self, out: &mut Vec<Sprite>) {
            self.debug().sprites(out);
        }

        fn palettes(&self, out: &mut Vec<Palette>) {
            self.debug().palettes(out);
        }

        fn print_log(&mut self) -> Option<&mut crate::debug::print::PrintLog> {
            self.debug_mut().print_log()
        }
    }
}

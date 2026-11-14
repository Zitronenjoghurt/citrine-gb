use crate::console::{Console, ConsoleKind, Keys, Timing, Touch, VideoSpec};
use crate::error::GbResult;
use crate::gb::{GameBoy, GbModel};
#[cfg(feature = "gba")]
use crate::gba::bios::{Bios, Boot};
#[cfg(feature = "gba")]
use crate::gba::machine::Gba;
#[cfg(feature = "nds")]
use crate::nds::machine::{Nds, NdsFirmware};
#[cfg(feature = "persistence")]
use crate::persistence::battery_save::BatterySave;
#[cfg(feature = "persistence")]
use crate::persistence::state::StateBody;
use crate::rom::Rom;
#[cfg(feature = "rom-id")]
use crate::rom::id::RomId;
use crate::traffic::Ring;
use std::sync::Arc;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Firmware {
    pub gb_boot_rom: Option<Vec<u8>>,
    #[cfg(feature = "gba")]
    pub gba_bios: Option<Bios>,
    #[cfg(feature = "gba")]
    pub gba_boot: Boot,
    #[cfg(feature = "nds")]
    pub nds: NdsFirmware,
}

pub enum Machine {
    Gb(Box<GameBoy>),
    #[cfg(feature = "gba")]
    Gba(Box<Gba>),
    #[cfg(feature = "nds")]
    Nds(Box<Nds>),
}

impl Machine {
    pub fn load(rom: &Rom, kind: ConsoleKind, firmware: &Firmware) -> GbResult<Self> {
        #[cfg(feature = "nds")]
        if kind == ConsoleKind::Nds {
            let nds = Nds::from_rom(rom, firmware.nds.clone())?;
            return Ok(Self::Nds(Box::new(nds)));
        }
        #[cfg(feature = "gba")]
        if kind == ConsoleKind::Gba {
            let bios = firmware.gba_bios.clone().unwrap_or_default();
            let gba = Gba::from_rom(rom, bios, firmware.gba_boot);
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
        #[cfg(feature = "nds")]
        if crate::persistence::state::is_nds_state(state) {
            return Nds::from_rom(rom, firmware.nds.clone())?
                .restore(state)
                .map(|nds| Self::Nds(Box::new(nds)));
        }
        if crate::persistence::state::is_gba_state(state) {
            #[cfg(feature = "gba")]
            return Gba::from_rom(
                rom,
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
        #[cfg(feature = "nds")]
        if let Self::Nds(nds) = self
            && crate::persistence::state::is_nds_state(state)
        {
            return nds.restore(state).map(|nds| Self::Nds(Box::new(nds)));
        }
        #[cfg(feature = "gba")]
        if let Self::Gba(gba) = self
            && crate::persistence::state::is_gba_state(state)
        {
            return gba.restore(state).map(|gba| Self::Gba(Box::new(gba)));
        }
        Self::from_state(state, &self.rom(), now, firmware)
    }

    pub fn rom(&self) -> Rom {
        let rom = Rom::from(self.rom_bytes());
        #[cfg(feature = "rom-id")]
        let rom = rom.with_ids((self.rom_id(), self.game_sha1()));
        rom
    }

    pub fn apply_cheats(&mut self, cheats: &[crate::cheats::Cheat]) {
        match self {
            Self::Gb(gb) => crate::cheats::apply(gb.as_mut(), cheats),
            #[cfg(feature = "gba")]
            Self::Gba(gba) => crate::cheats::apply(gba.as_mut(), cheats),
            #[cfg(feature = "nds")]
            Self::Nds(nds) => crate::cheats::apply(nds.as_mut(), cheats),
        }
    }

    #[cfg(feature = "persistence")]
    pub fn rewind_state(&self) -> GbResult<Vec<u8>> {
        let mut state = Vec::new();
        self.rewind_state_into(&mut state)?;
        Ok(state)
    }

    #[cfg(feature = "persistence")]
    pub fn rewind_state_into(&self, out: &mut Vec<u8>) -> GbResult<()> {
        use crate::console::Rollback;
        out.clear();
        match self {
            Self::Gb(gb) => gb.encode_state_into(out)?,
            #[cfg(feature = "gba")]
            Self::Gba(gba) => gba.encode_state_into(out)?,
            #[cfg(feature = "nds")]
            Self::Nds(nds) => return nds.encode_state_into(out),
        }
        out.extend_from_slice(self.frame());
        Ok(())
    }

    #[cfg(feature = "persistence")]
    pub fn rewind_to(&mut self, state: &[u8]) -> GbResult<()> {
        use crate::console::Rollback;
        let (body, pixels) = state.split_at(state.len().saturating_sub(self.frame().len()));
        match self {
            Self::Gb(gb) => {
                let mut back = gb.decode_state(body)?;
                gb.copy_state_from(&back);
                back.apu.muted = gb.apu.muted;
                back.apu.muted_channels = gb.apu.muted_channels;
                gb.apu = back.apu;
                gb.ppu.show_frame(pixels);
            }
            #[cfg(feature = "gba")]
            Self::Gba(gba) => {
                let back = gba.decode_state(body)?;
                gba.copy_state_from(&back);
                gba.bus_mut().ppu_mut().show_frame(pixels);
            }
            #[cfg(feature = "nds")]
            Self::Nds(nds) => nds.rewind_to(state)?,
        }
        Ok(())
    }

    pub fn as_gb(&self) -> Option<&GameBoy> {
        match self {
            Self::Gb(gb) => Some(gb),
            #[cfg(feature = "gba")]
            Self::Gba(_) => None,
            #[cfg(feature = "nds")]
            Self::Nds(_) => None,
        }
    }

    pub fn traffic(&self) -> Vec<Ring<'_>> {
        match self {
            Self::Gb(gb) => vec![Ring {
                name: "serial",
                traffic: gb.serial.traffic(),
            }],
            #[cfg(feature = "gba")]
            Self::Gba(gba) => vec![Ring {
                name: "serial",
                traffic: gba.bus().serial().traffic(),
            }],
            #[cfg(feature = "nds")]
            Self::Nds(nds) => nds.traffic(),
        }
    }

    pub fn traffic_clock(&self) -> u64 {
        match self {
            Self::Gb(_) => 0,
            #[cfg(feature = "gba")]
            Self::Gba(_) => 0,
            #[cfg(feature = "nds")]
            Self::Nds(nds) => nds.traffic_clock(),
        }
    }

    pub fn defer_3d(&mut self, on: bool) {
        #[cfg(feature = "nds")]
        if let Self::Nds(nds) = self {
            nds.defer_3d(on);
        }
        #[cfg(not(feature = "nds"))]
        let _ = on;
    }

    pub fn finish_picture(&mut self) -> bool {
        match self {
            #[cfg(feature = "nds")]
            Self::Nds(nds) => nds.finish_picture(),
            _ => false,
        }
    }

    pub fn hold_clock(&mut self, secs: f64) -> f64 {
        match self {
            Self::Gb(gb) => gb.cartridge.rtc_mut().map_or(secs, |rtc| rtc.hold(secs)),
            #[cfg(feature = "gba")]
            Self::Gba(gba) => gba.hold_clock(secs),
            #[cfg(feature = "nds")]
            Self::Nds(nds) => nds.hold_clock(secs),
        }
    }

    pub fn as_gb_mut(&mut self) -> Option<&mut GameBoy> {
        match self {
            Self::Gb(gb) => Some(gb),
            #[cfg(feature = "gba")]
            Self::Gba(_) => None,
            #[cfg(feature = "nds")]
            Self::Nds(_) => None,
        }
    }

    #[cfg(feature = "persistence")]
    pub fn set_muted(&mut self, muted: bool) {
        use crate::console::Rollback;
        match self {
            Self::Gb(gb) => gb.set_muted(muted),
            #[cfg(feature = "gba")]
            Self::Gba(gba) => gba.set_muted(muted),
            #[cfg(feature = "nds")]
            Self::Nds(nds) => nds.set_muted(muted),
        }
    }

    fn console(&self) -> &dyn Console {
        match self {
            Self::Gb(gb) => gb.as_ref(),
            #[cfg(feature = "gba")]
            Self::Gba(gba) => gba.as_ref(),
            #[cfg(feature = "nds")]
            Self::Nds(nds) => nds.as_ref(),
        }
    }

    fn console_mut(&mut self) -> &mut dyn Console {
        match self {
            Self::Gb(gb) => gb.as_mut(),
            #[cfg(feature = "gba")]
            Self::Gba(gba) => gba.as_mut(),
            #[cfg(feature = "nds")]
            Self::Nds(nds) => nds.as_mut(),
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

    fn set_touch(&mut self, touch: Option<Touch>) {
        self.console_mut().set_touch(touch);
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

    fn rom_bytes(&self) -> Arc<[u8]> {
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

    #[cfg(feature = "rom-id")]
    fn game_sha1(&self) -> [u8; 20] {
        self.console().game_sha1()
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
                #[cfg(feature = "nds")]
                Self::Nds(nds) => nds.as_ref(),
            }
        }

        fn debug_mut(&mut self) -> &mut dyn Debug {
            match self {
                Self::Gb(gb) => gb.as_mut(),
                #[cfg(feature = "gba")]
                Self::Gba(gba) => gba.as_mut(),
                #[cfg(feature = "nds")]
                Self::Nds(nds) => nds.as_mut(),
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

        fn cpu(&self) -> usize {
            self.debug().cpu()
        }

        fn select_cpu(&mut self, cpu: usize) {
            self.debug_mut().select_cpu(cpu);
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

const _: fn() = || {
    fn moves_between_threads<T: Send>() {}
    moves_between_threads::<Machine>();
};

#[cfg(test)]
mod tests {
    use super::{Firmware, Machine};
    #[cfg(all(feature = "debug", feature = "persistence"))]
    use crate::console::Keys;
    use crate::console::{Console, ConsoleKind};
    #[cfg(all(feature = "debug", feature = "persistence"))]
    use crate::debug::Debug;
    #[cfg(all(feature = "debug", feature = "persistence"))]
    use crate::debug::registers::RegisterGroup;
    #[cfg(all(feature = "debug", feature = "persistence"))]
    use crate::debug::step::{Step, Stop};
    #[cfg(all(feature = "debug", feature = "persistence"))]
    use crate::gb::{GameBoy, GbModel};
    #[cfg(all(feature = "debug", feature = "persistence"))]
    use crate::persistence::state::{GBA_STATE_MAGIC, SAVE_QUALITY, StateBody, state_kind};
    use crate::rom::Rom;
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    struct Counting;

    thread_local! {
        static ALLOCATED: Cell<usize> = const { Cell::new(0) };
    }

    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let _ = ALLOCATED.try_with(|bytes| bytes.set(bytes.get() + layout.size()));
            unsafe { System.alloc(layout) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    #[global_allocator]
    static COUNTING: Counting = Counting;

    fn allocated_by(run: impl FnOnce()) -> usize {
        let before = ALLOCATED.with(Cell::get);
        run();
        ALLOCATED.with(Cell::get) - before
    }

    fn reads_rom_without_copies(machine: &Machine, size: usize) {
        let bytes = allocated_by(|| {
            for _ in 0..16 {
                let rom = Rom::from(machine.rom_bytes());
                assert_eq!(rom.data.len(), size);
            }
        });
        assert_eq!(bytes, 0);
    }

    #[test]
    fn a_big_game_boy_rom_is_shared_not_copied() {
        let mut data = include_bytes!("../../citrine-gb-app/homebrew/tobu.gb").to_vec();
        data.resize(8 << 20, 0xFF);
        data[0x147] = 0x19;
        data[0x148] = 0x08;
        let rom = Rom::from(data);
        let mut machine = None;
        let bytes = allocated_by(|| {
            machine = Some(Machine::load(&rom, ConsoleKind::Gb, &Firmware::default()).unwrap());
        });
        assert!(bytes < 1 << 20, "{bytes} bytes to load");
        let machine = machine.unwrap();
        reads_rom_without_copies(&machine, 8 << 20);
        assert!(std::sync::Arc::ptr_eq(&machine.rom_bytes(), &rom.data));
    }

    #[cfg(all(feature = "gba", feature = "rom-id"))]
    #[test]
    fn a_running_game_hands_back_its_rom_with_its_ids() {
        let rom = Rom::from(vec![3u8; 0x8000]);
        let machine = Machine::load(&rom, ConsoleKind::Gba, &Firmware::default()).unwrap();
        let back = machine.rom();
        assert!(std::sync::Arc::ptr_eq(&back.data, &rom.data));
        assert_eq!(back.ids().0, rom.ids().0);
    }

    #[cfg(feature = "gba")]
    #[test]
    fn a_big_gba_rom_is_shared_not_copied() {
        let rom = Rom::from(vec![0; 32 << 20]);
        let mut machine = None;
        let bytes = allocated_by(|| {
            machine = Some(Machine::load(&rom, ConsoleKind::Gba, &Firmware::default()).unwrap());
        });
        assert!(bytes < 4 << 20, "{bytes} bytes to load");
        let machine = machine.unwrap();
        reads_rom_without_copies(&machine, 32 << 20);
        assert!(std::sync::Arc::ptr_eq(&machine.rom_bytes(), &rom.data));
    }

    #[test]
    fn a_rom_bigger_than_any_cartridge_clones_by_handle() {
        let rom = Rom::from(vec![0; 64 << 20]);
        let bytes = allocated_by(|| {
            for _ in 0..16 {
                assert_eq!(rom.clone().data.len(), 64 << 20);
            }
        });
        assert_eq!(bytes, 0);
    }

    #[cfg(all(feature = "debug", feature = "persistence"))]
    const TOBU: &[u8] = include_bytes!("../../citrine-gb-app/homebrew/tobu.gb");

    #[cfg(all(feature = "debug", feature = "persistence"))]
    const PETRIS: &[u8] = include_bytes!("../../citrine-gb-app/homebrew/petris.gbc");

    #[cfg(all(feature = "debug", feature = "persistence"))]
    const NOW: u64 = 1_700_000_000;

    #[cfg(all(feature = "debug", feature = "persistence"))]
    const FRAMES: u32 = 600;

    #[cfg(all(feature = "debug", feature = "persistence"))]
    fn keys_at(frame: u32) -> Keys {
        match frame % 120 {
            0..10 => Keys::START,
            30..40 => Keys::A,
            50..90 => Keys::RIGHT,
            95..100 => Keys::A | Keys::LEFT,
            _ => Keys::empty(),
        }
    }

    #[cfg(all(feature = "debug", feature = "persistence"))]
    fn game_boy(data: &[u8], model: GbModel) -> GameBoy {
        let mut gb = GameBoy::new_empty(model);
        gb.load_rom_with_model(&Rom::new(data), model)
            .expect("load");
        gb.apu.set_sample_rate(48_000);
        gb
    }

    #[cfg(all(feature = "debug", feature = "persistence"))]
    fn machine(data: &[u8], kind: ConsoleKind) -> Machine {
        let mut machine = Machine::load(&Rom::new(data), kind, &Firmware::default()).expect("load");
        machine.set_sample_rate(48_000);
        machine
    }

    #[cfg(all(feature = "debug", feature = "persistence"))]
    #[test]
    fn a_machine_plays_like_the_game_boy_it_holds() {
        let mut gb = game_boy(TOBU, GbModel::Dmg);
        let mut machine = machine(TOBU, ConsoleKind::Gb);
        let mut audio = Vec::new();
        let mut heard = 0;

        for frame in 0..FRAMES {
            gb.set_keys(keys_at(frame));
            machine.set_keys(keys_at(frame));
            gb.run_frame();
            machine.run_frame();

            assert_eq!(machine.frame(), gb.frame().as_slice(), "frame {frame}");
            machine.take_audio(&mut audio);
            let expected = std::mem::take(&mut gb.apu.audio_buffer);
            assert_eq!(audio, expected, "audio of frame {frame}");
            heard += audio.len();
            audio.clear();
        }

        assert!(heard > 0);
        assert_eq!(
            machine.save_state(NOW).expect("machine state"),
            gb.dump_full(NOW).expect("game boy state")
        );
    }

    #[cfg(all(feature = "debug", feature = "persistence"))]
    #[test]
    fn a_machine_from_a_state_goes_on_like_the_game_boy() {
        let mut gb = game_boy(TOBU, GbModel::Dmg);
        for frame in 0..300 {
            gb.set_keys(keys_at(frame));
            gb.run_frame();
        }
        let state = gb.dump_full(NOW).expect("state");

        let rom = Rom::new(TOBU);
        let mut machine =
            Machine::from_state(&state, &rom, NOW, &Firmware::default()).expect("machine");
        let mut again = GameBoy::from_dump(&state, &rom, NOW).expect("game boy");
        for frame in 300..FRAMES {
            machine.set_keys(keys_at(frame));
            again.set_keys(keys_at(frame));
            machine.run_frame();
            again.run_frame();
        }

        assert_eq!(machine.frame(), again.frame().as_slice());
        assert_eq!(
            machine.save_state(NOW).expect("machine state"),
            again.dump_full(NOW).expect("game boy state")
        );
    }

    #[cfg(all(feature = "debug", feature = "persistence"))]
    #[test]
    fn a_game_boy_describes_its_console() {
        let dmg = machine(TOBU, ConsoleKind::Gb);
        let cgb = machine(PETRIS, ConsoleKind::Gbc);

        assert_eq!(dmg.kind(), ConsoleKind::Gb);
        assert_eq!(cgb.kind(), ConsoleKind::Gbc);
        assert_eq!((dmg.video().width, dmg.video().height), (160, 144));
        assert_eq!(dmg.timing().clock_hz, 4_194_304);
        assert_eq!(dmg.timing().cycles_per_frame, 70_224);
        assert_eq!(
            dmg.audio_channels(),
            ["Pulse 1", "Pulse 2", "Wave", "Noise"]
        );
        assert_eq!(dmg.rom_id(), Rom::new(TOBU).header().expect("header").id());
        assert!(dmg.as_gb().is_some_and(|gb| gb.model == GbModel::Dmg));
        assert!(cgb.as_gb().is_some_and(|gb| gb.model == GbModel::Cgb));
    }

    #[cfg(all(feature = "debug", feature = "persistence"))]
    #[cfg(not(feature = "gba"))]
    #[test]
    fn a_gba_game_does_not_load_into_a_machine() {
        assert!(Machine::load(&Rom::new(TOBU), ConsoleKind::Gba, &Firmware::default()).is_err());
    }

    #[cfg(all(feature = "debug", feature = "persistence"))]
    #[cfg(feature = "gba")]
    #[test]
    fn a_gba_game_loads_into_a_gba_machine() {
        let mut machine = Machine::load(
            &Rom::new(&[0xFE, 0xFF, 0xFF, 0xEA]),
            ConsoleKind::Gba,
            &Firmware::default(),
        )
        .expect("load");
        machine.run_frame();
        assert_eq!(machine.kind(), ConsoleKind::Gba);
        assert_eq!(machine.video().width, 240);
        assert_eq!(machine.frame().len(), 240 * 160 * 4);
        assert!(machine.as_gb().is_none());
    }

    #[cfg(all(feature = "debug", feature = "persistence"))]
    #[cfg(feature = "nds")]
    #[test]
    fn a_ds_game_loads_as_a_ds_with_both_screens() {
        let rom = Rom::from(crate::rom::nds::tests::nds_rom(&["Test"]));
        let machine = Machine::load(&rom, ConsoleKind::Nds, &Firmware::default()).expect("load");
        assert_eq!(machine.kind(), ConsoleKind::Nds);
        assert_eq!(machine.frame().len(), 256 * 384 * 4);
        assert_eq!(machine.title(), "CITRINE TEST");
        assert!(machine.as_gb().is_none());
    }

    #[cfg(all(feature = "debug", feature = "persistence"))]
    #[cfg(not(feature = "nds"))]
    #[test]
    fn a_ds_game_without_the_ds_core_says_so() {
        let rom = Rom::from(crate::rom::nds::tests::nds_rom(&[]));
        let loaded = Machine::load(&rom, ConsoleKind::Nds, &Firmware::default());
        assert!(matches!(loaded, Err(crate::error::GbError::NdsGame)));
    }

    #[cfg(all(feature = "debug", feature = "persistence"))]
    #[cfg(feature = "nds")]
    #[test]
    fn a_file_without_a_ds_header_is_no_ds_game() {
        let loaded = Machine::load(
            &Rom::new(&[0; 0x100]),
            ConsoleKind::Nds,
            &Firmware::default(),
        );
        assert!(matches!(loaded, Err(crate::error::GbError::NotNds)));
    }

    #[cfg(all(feature = "debug", feature = "persistence"))]
    #[test]
    fn a_boot_rom_from_the_firmware_runs_first() {
        let firmware = Firmware {
            gb_boot_rom: Some(vec![0x00; 0x100]),
            #[cfg(feature = "gba")]
            gba_bios: None,
            #[cfg(feature = "gba")]
            gba_boot: crate::gba::bios::Boot::Skip,
            #[cfg(feature = "nds")]
            nds: crate::nds::machine::NdsFirmware::default(),
        };
        let machine = Machine::load(&Rom::new(TOBU), ConsoleKind::Gb, &firmware).expect("load");
        let gb = machine.as_gb().expect("game boy");

        assert!(gb.boot_rom.mounted);
        assert_eq!(gb.cpu.pc, 0x0000);
    }

    #[cfg(all(feature = "debug", feature = "persistence"))]
    #[test]
    fn a_muted_channel_is_muted_through_the_machine() {
        let mut machine = machine(TOBU, ConsoleKind::Gb);
        machine.set_channel_enabled(2, false);

        let gb = machine.as_gb().expect("game boy");
        assert!(!gb.apu.channel_enabled(2));
        assert!(gb.apu.channel_enabled(1));
    }

    #[cfg(all(feature = "debug", feature = "persistence"))]
    #[test]
    fn the_debug_view_of_a_machine_is_its_game_boy() {
        let mut gb = game_boy(TOBU, GbModel::Dmg);
        let mut machine = machine(TOBU, ConsoleKind::Gb);
        gb.debugger.record_channels = true;
        machine.record_waves(true);
        gb.debugger.static_analysis_enabled = true;
        machine.set_code_analysis(true);
        assert!(machine.code_analysis());
        for frame in 0..FRAMES {
            gb.set_keys(keys_at(frame));
            machine.set_keys(keys_at(frame));
            gb.run_frame();
            machine.run_frame();
        }

        let mut expected: Vec<RegisterGroup> = Vec::new();
        gb.registers(&mut expected);
        let mut groups = Vec::new();
        Debug::registers(&machine, &mut groups);
        assert_eq!(groups, expected);

        assert_eq!(Debug::pc(&machine), gb.pc_address());
        assert_eq!(Debug::code_rows(&machine), gb.code_rows());
        assert!(gb.code_rows() > 0);
        let mut lines = Vec::new();
        let mut expected_lines = Vec::new();
        Debug::disassemble(&machine, 0..gb.code_rows(), &mut lines);
        gb.disassemble(0..gb.code_rows(), &mut expected_lines);
        assert_eq!(lines, expected_lines);
        let pc = gb.pc_address().expect("pc");
        assert_eq!(Debug::code_row_of(&machine, pc), gb.code_row_of(pc));

        assert_eq!(Debug::cycles(&machine), gb.debugger.total_cycles);
        assert!(!gb.debugger.waves().is_empty());
        assert_eq!(machine.waves(), gb.debugger.waves());
    }

    #[cfg(all(feature = "debug", feature = "persistence"))]
    #[test]
    fn breakpoints_set_through_the_machine_stop_its_game_boy() {
        let mut machine = machine(TOBU, ConsoleKind::Gb);
        machine.run_frame();
        let pc = Debug::pc(&machine).expect("pc");
        machine.breakpoints().insert(pc);

        let gb = machine.as_gb().expect("game boy");
        assert!(gb.debugger.breakpoints.contains(&pc));
    }

    #[cfg(all(feature = "debug", feature = "persistence"))]
    #[test]
    fn a_game_boy_has_its_interrupt_vectors_and_a_short_pc() {
        let machine = machine(TOBU, ConsoleKind::Gb);
        let features = machine.features();

        assert_eq!(features.pc_digits, 4);
        assert!(!features.thumb);
        assert_eq!(
            features.vectors,
            [
                ("VBlank", 0x40),
                ("STAT", 0x48),
                ("Timer", 0x50),
                ("Serial", 0x58),
                ("Joypad", 0x60),
            ]
        );
    }

    #[cfg(all(feature = "debug", feature = "persistence"))]
    #[test]
    fn a_step_through_the_machine_steps_its_game_boy() {
        let mut gb = game_boy(TOBU, GbModel::Dmg);
        let mut machine = machine(TOBU, ConsoleKind::Gb);
        gb.run_frame();
        machine.run_frame();
        let before = Debug::cycles(&machine);

        assert_eq!(Debug::step(&mut machine, Step::Instruction), Stop::Finished);
        assert_eq!(gb.run_step(Step::Instruction), Stop::Finished);

        assert!(Debug::cycles(&machine) > before);
        assert_eq!(Debug::cycles(&machine), Debug::cycles(&gb));
        assert_eq!(Debug::pc(&machine), Debug::pc(&gb));
    }

    #[cfg(all(feature = "debug", feature = "persistence"))]
    fn cart(cartridge_type: u8, ram_size_code: u8) -> Machine {
        let mut data = vec![0u8; 0x8000];
        data[0x0134..0x0143].copy_from_slice(b"CITRINE CARTRGE");
        data[0x0147] = cartridge_type;
        data[0x0149] = ram_size_code;
        machine(&data, ConsoleKind::Gb)
    }

    #[cfg(all(feature = "debug", feature = "persistence"))]
    #[test]
    fn a_machine_knows_its_rom_title_battery_and_clock() {
        let tobu = machine(TOBU, ConsoleKind::Gb);
        assert_eq!(
            tobu.rom_bytes(),
            game_boy(TOBU, GbModel::Dmg).cartridge.rom_bytes()
        );
        assert!(!tobu.has_clock());

        let plain = cart(0x03, 0x02);
        assert_eq!(plain.title(), "CITRINE CARTRGE");
        assert!(plain.has_battery());
        assert!(!plain.has_clock());

        let clock = cart(0x10, 0x02);
        assert!(clock.has_battery());
        assert!(clock.has_clock());
    }

    #[cfg(all(feature = "debug", feature = "persistence"))]
    #[test]
    fn a_state_body_packs_to_the_saved_state() {
        let mut machine = machine(PETRIS, ConsoleKind::Gbc);
        for frame in 0..120 {
            machine.set_keys(keys_at(frame));
            machine.run_frame();
        }
        let body = machine.state_body(NOW).expect("body");
        assert_eq!(body.kind, ConsoleKind::Gbc);
        assert_eq!(
            body.pack(SAVE_QUALITY).expect("pack"),
            machine.save_state(NOW).expect("state")
        );
    }

    #[cfg(all(feature = "debug", feature = "persistence"))]
    #[test]
    fn a_state_tells_its_console() {
        let mut dmg = machine(TOBU, ConsoleKind::Gb);
        let mut cgb = machine(PETRIS, ConsoleKind::Gbc);
        let mut cgb_as_dmg = machine(PETRIS, ConsoleKind::Gb);
        assert_eq!(
            state_kind(&dmg.save_state(NOW).unwrap()),
            Some(ConsoleKind::Gb)
        );
        assert_eq!(
            state_kind(&cgb.save_state(NOW).unwrap()),
            Some(ConsoleKind::Gbc)
        );
        assert_eq!(
            state_kind(&cgb_as_dmg.save_state(NOW).unwrap()),
            Some(ConsoleKind::Gb)
        );
        assert_eq!(state_kind(b"not a state"), None);
    }

    #[cfg(all(feature = "debug", feature = "persistence"))]
    #[test]
    fn a_gba_state_starts_with_its_magic_and_version() {
        let body = StateBody {
            kind: ConsoleKind::Gba,
            data: b"gba body".to_vec(),
        };
        let state = body.pack(SAVE_QUALITY).expect("pack");
        assert_eq!(state[..4], GBA_STATE_MAGIC);
        assert_eq!(state[4], 1);
        assert_eq!(state_kind(&state), Some(ConsoleKind::Gba));
    }

    #[cfg(all(feature = "debug", feature = "persistence"))]
    #[test]
    fn a_game_boy_state_has_no_magic() {
        let mut machine = machine(TOBU, ConsoleKind::Gb);
        let state = machine.save_state(NOW).expect("state");
        assert_ne!(state[..4], GBA_STATE_MAGIC);
        let body = machine.state_body(NOW).expect("body");
        assert_eq!(body.pack(SAVE_QUALITY).expect("pack"), state);
    }

    #[cfg(all(feature = "debug", feature = "persistence"))]
    #[cfg(not(feature = "gba"))]
    #[test]
    fn a_gba_state_does_not_load_as_a_game_boy() {
        let state = StateBody {
            kind: ConsoleKind::Gba,
            data: Vec::new(),
        }
        .pack(SAVE_QUALITY)
        .expect("pack");
        let refused = Machine::from_state(&state, &Rom::new(TOBU), NOW, &Firmware::default())
            .err()
            .expect("refused");
        assert!(matches!(refused, crate::error::GbError::GbaGame));
    }
}

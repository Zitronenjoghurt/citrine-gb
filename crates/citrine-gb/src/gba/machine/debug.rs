use super::{CYCLES_PER_FRAME, Gba};
use crate::debug::breakpoints::Breakpoints;
use crate::debug::code::{Address, Line};
use crate::debug::memory::MemoryBlock;
use crate::debug::print::PrintLog;
use crate::debug::registers::RegisterGroup;
use crate::debug::step::{Step, Stop};
use crate::debug::trace::TraceLog;
use crate::debug::video::{Palette, SheetColors, Sprite, TileMap, TileSheet};
use crate::debug::{Debug, Features};
use crate::gba::cpu::Cpu;
use crate::gba::cpu::debug::FEATURES;
use crate::gba::cpu::step::Stepper;
use crate::gba::disasm::{Pc, code_address, lines};
use std::collections::VecDeque;
use std::ops::Range;

const STEP_BUDGET: u32 = (CYCLES_PER_FRAME * 2) as u32;

impl Gba {
    pub fn take_breakpoint_hit(&mut self) -> bool {
        std::mem::take(&mut self.debugger.hit_breakpoint)
    }

    pub fn step_pending(&self) -> bool {
        self.stepping.pending()
    }

    pub fn resume_step(&mut self) -> Stop {
        let mut stepping = std::mem::take(&mut self.stepping);
        let stop = stepping.resume(self, STEP_BUDGET);
        self.stepping = stepping;
        stop
    }

    pub(super) fn drain_waves(&mut self) {
        for levels in self.bus.sound_mut().take_taps() {
            self.debugger.push_waves(&levels);
        }
    }

    pub(super) fn on_fetch(&mut self) {
        if self.debugger.static_analysis_enabled {
            let pc = Pc {
                addr: self.cpu.pc(),
                thumb: self.cpu.thumb(),
            };
            self.code.on_fetch(&self.bus, pc);
        }
    }

    pub(super) fn breakpoint_after(&mut self, before: u32) -> bool {
        let hit = self.cpu.pc() != before && self.breakpoint().is_some();
        self.debugger.hit_breakpoint |= hit;
        hit
    }

    fn code_address(&self, addr: u32) -> Option<Address> {
        code_address(addr, self.bus.rom_len())
    }
}

impl Stepper for Gba {
    fn cpu(&self) -> &Cpu {
        &self.cpu
    }

    fn step_instruction(&mut self) -> u32 {
        let mut used = 0;
        loop {
            let dma = self.bus.dma_active() && !self.bus.power().running();
            let cycles = Gba::step(self);
            used += cycles;
            if !dma || cycles == 0 {
                return used;
            }
        }
    }

    fn breakpoint(&self) -> Option<Address> {
        if self.debugger.breakpoints.is_empty() {
            return None;
        }
        self.code_address(self.cpu.pc())
            .filter(|at| self.debugger.breakpoints.contains(at))
    }

    fn line(&self) -> u16 {
        self.bus.ppu().vcount()
    }

    fn frame(&self) -> u64 {
        self.cycles / CYCLES_PER_FRAME
    }
}

impl Debug for Gba {
    fn features(&self) -> Features {
        FEATURES
    }

    fn registers(&self, out: &mut Vec<RegisterGroup>) {
        self.cpu.registers(out);
        self.io_registers(out);
    }

    fn memory_blocks(&self, out: &mut Vec<MemoryBlock>) {
        self.gba_memory_blocks(out);
    }

    fn locate(&self, addr: u32) -> Option<Address> {
        self.gba_locate(addr)
    }

    fn peek(&self, at: Address, out: &mut [u8]) {
        self.gba_peek(at, out);
    }

    fn poke(&mut self, at: Address, bytes: &[u8]) {
        self.gba_poke(at, bytes);
    }

    fn pc(&self) -> Option<Address> {
        self.code_address(self.cpu.pc())
    }

    fn code_analysis(&self) -> bool {
        self.debugger.static_analysis_enabled
    }

    fn set_code_analysis(&mut self, on: bool) {
        self.debugger.static_analysis_enabled = on;
    }

    fn code_rows(&self) -> usize {
        self.code.rows()
    }

    fn code_row_of(&self, at: Address) -> Option<usize> {
        self.code.row_of(at)
    }

    fn disassemble(&self, rows: Range<usize>, out: &mut Vec<Line>) {
        lines(&self.code, &self.bus, rows, out);
    }

    fn breakpoints(&mut self) -> &mut Breakpoints {
        &mut self.debugger.breakpoints
    }

    fn cycles(&self) -> u128 {
        u128::from(self.cycles)
    }

    fn record_waves(&mut self, on: bool) {
        self.debugger.record_channels = on;
        self.bus.sound_mut().record_taps(on);
    }

    fn waves(&self) -> &[VecDeque<f32>] {
        self.debugger.waves()
    }

    fn step(&mut self, step: Step) -> Stop {
        let mut stepping = std::mem::take(&mut self.stepping);
        let stop = stepping.start(self, step, STEP_BUDGET);
        self.stepping = stepping;
        stop
    }

    fn step_pending(&self) -> bool {
        Gba::step_pending(self)
    }

    fn resume_step(&mut self) -> Stop {
        Gba::resume_step(self)
    }

    fn cancel_step(&mut self) {
        self.stepping.cancel();
    }

    fn take_breakpoint_hit(&mut self) -> bool {
        Gba::take_breakpoint_hit(self)
    }

    fn frame_cycles(&self) -> u32 {
        CYCLES_PER_FRAME as u32
    }

    fn scanline(&self) -> u16 {
        self.bus.ppu().vcount()
    }

    fn trace(&mut self) -> &mut TraceLog {
        &mut self.debugger.trace
    }

    fn tile_sheets(&self, colors: SheetColors, out: &mut Vec<TileSheet>) {
        Gba::tile_sheets(self, colors, out);
    }

    fn tile_maps(&self, out: &mut Vec<TileMap>) {
        Gba::tile_maps(self, out);
    }

    fn sprites(&self, out: &mut Vec<Sprite>) {
        Gba::sprites(self, out);
    }

    fn palettes(&self, out: &mut Vec<Palette>) {
        Gba::palettes(self, out);
    }

    fn print_log(&mut self) -> Option<&mut PrintLog> {
        Some(self.bus.debug_print_mut().log_mut())
    }
}

mod io;
mod memory;

#[cfg(test)]
mod tests {
    use crate::debug::Debug;
    use crate::debug::code::{Address, BlockId, GBA_BANK};
    use crate::debug::step::{Step, Stop};
    use crate::gba::machine::CYCLES_PER_FRAME;
    use crate::gba::machine::Gba;

    const LOOP: u32 = 0xEAFF_FFFE;
    const ROM: u32 = 0x0800_0000;

    fn rom(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|word| word.to_le_bytes()).collect()
    }

    fn call_program() -> Gba {
        Gba::new(&rom(&[
            0xE3A0_0001,
            0xEB00_0001,
            0xE3A0_1002,
            LOOP,
            0xE3A0_2003,
            0xE12F_FF1E,
        ]))
    }

    fn cart(offset: u32) -> Address {
        Address::gba(BlockId::GamePak, offset)
    }

    fn texts(gba: &Gba) -> Vec<String> {
        let mut out = Vec::new();
        gba.disassemble(0..gba.code_rows(), &mut out);
        out.iter()
            .map(|line| format!("{} {}", line.at, line.text()))
            .collect()
    }

    #[test]
    fn the_pc_names_the_cartridge_entry() {
        let gba = call_program();
        assert_eq!(Debug::pc(&gba), Some(cart(0)));
        assert_eq!(gba.features().pc_digits, 8);
    }

    #[test]
    fn registers_list_every_bank() {
        let gba = call_program();
        let mut groups = Vec::new();
        gba.registers(&mut groups);
        let names: Vec<&str> = groups.iter().map(|group| group.name).collect();
        assert_eq!(
            names,
            [
                "CPU",
                "User and System",
                "FIQ",
                "Supervisor",
                "Abort",
                "IRQ",
                "Undefined",
                "Display",
                "DMA",
                "Timers",
                "Interrupts",
                "System"
            ]
        );
    }

    #[test]
    fn code_analysis_maps_the_code_the_machine_runs() {
        let mut gba = call_program();
        gba.run_frame();
        assert_eq!(gba.code_rows(), 0);
        let mut gba = call_program();
        gba.set_code_analysis(true);
        assert!(gba.code_analysis());
        gba.run_frame();
        assert_eq!(
            texts(&gba),
            [
                "08000000 MOV R0, #0x1",
                "08000004 BL 08000010",
                "08000008 MOV R1, #0x2",
                "0800000C B 0800000C",
                "08000010 MOV R2, #0x3",
                "08000014 BX LR",
            ]
        );
        assert_eq!(gba.code_row_of(cart(0x10)), Some(4));
    }

    #[test]
    fn step_over_runs_the_call_and_stops_after_it() {
        let mut gba = call_program();
        assert_eq!(Debug::step(&mut gba, Step::Instruction), Stop::Finished);
        assert_eq!(gba.cpu().pc(), ROM + 4);
        assert_eq!(Debug::step(&mut gba, Step::Over), Stop::Finished);
        assert_eq!(gba.cpu().pc(), ROM + 8);
        assert_eq!(gba.cpu().reg(2), 3);
    }

    #[test]
    fn step_out_returns_from_the_call() {
        let mut gba = call_program();
        Debug::step(&mut gba, Step::Instruction);
        Debug::step(&mut gba, Step::Instruction);
        assert_eq!(gba.cpu().pc(), ROM + 0x10);
        assert_eq!(Debug::step(&mut gba, Step::Out), Stop::Finished);
        assert_eq!(gba.cpu().pc(), ROM + 8);
    }

    #[test]
    fn a_step_over_stops_at_a_breakpoint_inside_the_call() {
        let mut gba = call_program();
        Debug::step(&mut gba, Step::Instruction);
        gba.breakpoints().insert(cart(0x14));
        assert_eq!(
            Debug::step(&mut gba, Step::Over),
            Stop::Breakpoint(cart(0x14))
        );
        assert_eq!(gba.cpu().pc(), ROM + 0x14);
    }

    #[test]
    fn line_and_frame_steps_end_on_the_next_line_and_frame() {
        let mut gba = call_program();
        let line = gba.bus().ppu().vcount();
        assert_eq!(Debug::step(&mut gba, Step::Line), Stop::Finished);
        assert_eq!(gba.bus().ppu().vcount(), (line + 1) % 228);
        let frame = gba.cycles() / CYCLES_PER_FRAME;
        assert_eq!(Debug::step(&mut gba, Step::Frame), Stop::Finished);
        assert_eq!(gba.cycles() / CYCLES_PER_FRAME, frame + 1);
    }

    #[test]
    fn a_breakpoint_stops_the_running_frame_once() {
        let mut gba = call_program();
        gba.breakpoints().insert(cart(0x10));
        gba.run_frame();
        assert!(gba.take_breakpoint_hit());
        assert!(!gba.take_breakpoint_hit());
        assert_eq!(gba.cpu().pc(), ROM + 0x10);
        gba.run_frame();
        assert!(!gba.take_breakpoint_hit());
        assert!(gba.cycles() >= 280_896);
    }

    fn flash_game() -> Gba {
        let mut bytes = rom(&[LOOP]);
        bytes.resize(0x1_0010, 0);
        bytes[0x100..0x10C].copy_from_slice(b"FLASH1M_V103");
        Gba::new(&bytes)
    }

    fn enter_flash_id(gba: &mut Gba) {
        for (addr, value) in [(0x5555, 0xAA), (0x2AAA, 0x55), (0x5555, 0x90)] {
            gba.bus_mut().write8(0x0E00_0000 + addr, value);
        }
    }

    #[test]
    fn every_gba_block_is_in_the_memory_tool() {
        let gba = flash_game();
        let mut blocks = Vec::new();
        gba.memory_blocks(&mut blocks);
        let names: Vec<(&str, u32, u16)> = blocks
            .iter()
            .map(|block| (block.name, block.start, block.banks))
            .collect();
        assert_eq!(
            names,
            [
                ("BIOS", 0x0000_0000, 1),
                ("EWRAM", 0x0200_0000, 1),
                ("IWRAM", 0x0300_0000, 1),
                ("I/O", 0x0400_0000, 1),
                ("Palette", 0x0500_0000, 1),
                ("VRAM", 0x0600_0000, 1),
                ("OAM", 0x0700_0000, 1),
                ("ROM", 0x0800_0000, 2),
                ("ROM wait state 1", 0x0A00_0000, 2),
                ("ROM wait state 2", 0x0C00_0000, 2),
                ("Save", 0x0E00_0000, 2),
            ]
        );
        assert_eq!(
            Address::at(BlockId::GamePak, 1, 0x10).to_string(),
            "08010010"
        );
    }

    #[test]
    fn peek_reads_what_the_bus_reads_and_changes_nothing() {
        let mut gba = flash_game();
        for (addr, value) in [
            (0x0200_0010, 0x12),
            (0x0300_7F00, 0x34),
            (0x0500_0002, 0x56),
            (0x0600_0100, 0x78),
            (0x0700_0004, 0x9A),
            (0x0400_0200, 0x01),
            (0x0400_0004, 0x08),
        ] {
            gba.bus_mut().write16(addr, value);
        }
        gba.run_frame();
        enter_flash_id(&mut gba);
        let if_before = gba.bus().read16(0x0400_0202);
        let id = gba.bus().read8(0x0E00_0000);
        assert_ne!(id, 0xFF);

        let mut blocks = Vec::new();
        gba.memory_blocks(&mut blocks);
        for block in &blocks {
            for bank in [block.first_bank, block.last_bank()] {
                let mut bytes = vec![0; block.size as usize];
                gba.peek(Address::at(block.id, bank, 0), &mut bytes);
                let base = block.start + u32::from(bank) * GBA_BANK;
                let expected: Vec<u8> = if block.id == BlockId::GbaSave {
                    let start = (u32::from(bank) * GBA_BANK) as usize;
                    gba.bus().backup().bytes()[start..start + bytes.len()].to_vec()
                } else {
                    (0..block.size)
                        .map(|offset| gba.bus().read8(base + offset))
                        .collect()
                };
                assert!(bytes == expected, "{} bank {bank}", block.name);
            }
        }

        assert_eq!(gba.bus().read16(0x0400_0202), if_before);
        assert_ne!(if_before, 0);
        assert_eq!(gba.bus().read8(0x0E00_0000), id);
        assert_eq!(
            gba.locate(0x0801_0004),
            Some(Address::at(BlockId::GamePak, 1, 4))
        );
    }

    #[test]
    fn poke_writes_ram_and_vram_bytes_and_leaves_rom_alone() {
        let mut gba = flash_game();
        gba.poke(Address::gba(BlockId::Ewram, 3), &[0xAB]);
        gba.poke(Address::gba(BlockId::GbaVram, 0x101), &[0xCD]);
        gba.poke(Address::gba(BlockId::GbaOam, 2), &[0xEF]);
        gba.poke(Address::gba(BlockId::GamePak, 0), &[0x00]);

        assert_eq!(gba.bus().read8(0x0200_0003), 0xAB);
        assert_eq!(gba.bus().read16(0x0600_0100), 0xCD00);
        assert_eq!(gba.bus().read8(0x0700_0002), 0xEF);
        assert_eq!(gba.bus().read32(0x0800_0000), LOOP);
    }

    #[test]
    fn the_io_groups_name_their_registers_and_fields() {
        let mut gba = call_program();
        gba.bus_mut().write16(0x0400_0000, 0x0403);
        let mut groups = Vec::new();
        gba.registers(&mut groups);
        let display = groups.iter().find(|group| group.name == "Display").unwrap();
        let dispcnt = display.rows[0];
        assert_eq!(dispcnt.name, "DISPCNT");
        assert_eq!(dispcnt.field("BG mode"), Some(3));
        assert_eq!(dispcnt.field("BG2"), Some(1));
        let names: Vec<&str> = groups
            .iter()
            .flat_map(|group| group.rows.iter().map(|row| row.name))
            .collect();
        for wanted in [
            "DISPSTAT",
            "VCOUNT",
            "BG3CNT",
            "DMA3SAD",
            "DMA0CNT_H",
            "TM2CNT_H",
            "IE",
            "IF",
            "IME",
            "WAITCNT",
            "KEYINPUT",
        ] {
            assert!(names.contains(&wanted), "{wanted}");
        }
    }

    pub(crate) const PRINT_HI: [u32; 16] = [
        0xE3A0_0404,
        0xE280_08FF,
        0xE280_0CF6,
        0xE3A0_1CC0,
        0xE281_10DE,
        0xE280_2E18,
        0xE1C2_10B0,
        0xE3A0_3048,
        0xE5C0_3000,
        0xE3A0_3069,
        0xE5C0_3001,
        0xE280_2C01,
        0xE3A0_1C01,
        0xE281_1003,
        0xE1C2_10B0,
        LOOP,
    ];

    #[test]
    fn a_program_prints_lines_through_the_mgba_debug_registers() {
        use crate::debug::print::{PrintLevel, PrintLine};
        use crate::gba::wait::Width;
        let mut gba = Gba::new(&rom(&PRINT_HI));
        gba.run_frame();

        let lines: Vec<PrintLine> = gba.print_log().unwrap().lines().iter().cloned().collect();
        assert_eq!(
            lines,
            [PrintLine {
                level: PrintLevel::Info,
                text: "Hi".into()
            }]
        );
        let enabled = gba.bus_mut().cpu_read(0x04FF_F780, Width::Half);
        assert_eq!(enabled, 0x1DEA);
    }

    #[test]
    fn the_debug_print_stays_off_until_the_game_enables_it() {
        let mut words = PRINT_HI;
        words[6] = 0xE1A0_0000;
        let mut gba = Gba::new(&rom(&words));
        gba.run_frame();

        assert!(gba.print_log().unwrap().lines().is_empty());
    }
}

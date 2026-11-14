use crate::ReadMemory;
use crate::debug::code::{Address, BlockId};
use crate::gb::GameBoy;
use crate::gb::boot_rom::BootRom;
use crate::gb::cartridge::Cartridge;
use crate::gb::memory::Memory;
use crate::gb::ppu::Ppu;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryBlock {
    pub id: BlockId,
    pub name: &'static str,
    pub start: u32,
    pub size: u32,
    pub first_bank: u16,
    pub banks: u16,
    pub writable: bool,
}

impl MemoryBlock {
    pub fn holds(&self, bank: u16) -> bool {
        bank >= self.first_bank && bank - self.first_bank < self.banks
    }

    pub fn contains(&self, at: Address) -> bool {
        at.block == self.id && self.holds(at.bank) && at.offset < self.size
    }

    pub fn last_bank(&self) -> u16 {
        self.first_bank + self.banks.max(1) - 1
    }
}

const fn block(id: BlockId, name: &'static str, start: u32, size: u32) -> MemoryBlock {
    MemoryBlock {
        id,
        name,
        start,
        size,
        first_bank: 0,
        banks: 1,
        writable: true,
    }
}

pub(crate) fn address_of(
    boot_rom: &BootRom,
    cartridge: &Cartridge,
    ppu: &Ppu,
    memory: &Memory,
    addr: u16,
) -> Option<Address> {
    let offset = |base: u16| u32::from(addr - base);
    match addr {
        _ if boot_rom.maps(addr) => Some(Address::at(BlockId::BootRom, 0, offset(0))),
        0x0000..=0x7FFF => {
            use crate::debug::disassembly::DisassemblySource;
            Some(cartridge.rom_address(addr))
        }
        0x8000..=0x9FFF => Some(Address::at(BlockId::Vram, ppu.vram_bank(), offset(0x8000))),
        0xA000..=0xBFFF => cartridge.ram_address(addr),
        0xC000..=0xFDFF => memory.code_address(addr),
        0xFE00..=0xFE9F => Some(Address::at(BlockId::Oam, 0, offset(0xFE00))),
        0xFEA0..=0xFEFF => None,
        0xFF00..=0xFF7F => Some(Address::at(BlockId::Io, 0, offset(0xFF00))),
        0xFF80..=0xFFFF => Some(Address::hram(offset(0xFF80))),
    }
}

impl GameBoy {
    pub fn memory_blocks(&self, out: &mut Vec<MemoryBlock>) {
        out.clear();
        let rom_banks = self.cartridge.rom_banks().max(2) as u16;
        out.push(MemoryBlock {
            writable: false,
            ..block(BlockId::Rom, "ROM bank 0", 0x0000, 0x4000)
        });
        out.push(MemoryBlock {
            first_bank: 1,
            banks: rom_banks - 1,
            writable: false,
            ..block(BlockId::Rom, "Switchable ROM", 0x4000, 0x4000)
        });
        out.push(MemoryBlock {
            banks: if self.model.is_cgb() { 2 } else { 1 },
            ..block(BlockId::Vram, "VRAM", 0x8000, 0x2000)
        });
        let ram_banks = self.cartridge.ram_banks().len() as u16;
        if ram_banks > 0 {
            out.push(MemoryBlock {
                banks: ram_banks,
                ..block(BlockId::CartRam, "Cart RAM", 0xA000, 0x2000)
            });
        }
        out.push(block(BlockId::Wram, "WRAM bank 0", 0xC000, 0x1000));
        let wram_banks = self.memory.wram_banks() as u16;
        let name = if wram_banks > 2 {
            "Switchable WRAM"
        } else {
            "WRAM bank 1"
        };
        out.push(MemoryBlock {
            first_bank: 1,
            banks: wram_banks.saturating_sub(1).max(1),
            ..block(BlockId::Wram, name, 0xD000, 0x1000)
        });
        out.push(block(BlockId::Oam, "OAM", 0xFE00, 0xA0));
        out.push(block(BlockId::Io, "I/O", 0xFF00, 0x80));
        out.push(block(BlockId::Hram, "HRAM", 0xFF80, 0x80));
        if self.boot_rom.mounted {
            out.push(MemoryBlock {
                writable: false,
                ..block(
                    BlockId::BootRom,
                    "Boot ROM",
                    0x0000,
                    self.boot_rom.rom.len() as u32,
                )
            });
        }
    }

    pub fn address_of(&self, addr: u16) -> Option<Address> {
        address_of(
            &self.boot_rom,
            &self.cartridge,
            &self.ppu,
            &self.memory,
            addr,
        )
    }

    pub fn peek(&self, at: Address, out: &mut [u8]) {
        let bank = usize::from(at.bank);
        let bytes = match at.block {
            BlockId::BootRom => Some(&self.boot_rom.rom[..]),
            BlockId::Rom => self.cartridge.rom_bank(bank),
            BlockId::Vram => self.ppu.vram(bank),
            BlockId::CartRam => self.cartridge.ram_banks().get(bank).map(|b| &b[..]),
            BlockId::Wram => self.memory.wram_bank(bank).map(|b| &b[..]),
            BlockId::Oam => Some(self.ppu.oam()),
            BlockId::Io | BlockId::Hram => None,
            BlockId::Bios
            | BlockId::Ewram
            | BlockId::Iwram
            | BlockId::GbaIo
            | BlockId::Palette
            | BlockId::GbaVram
            | BlockId::GbaOam
            | BlockId::GamePak
            | BlockId::GamePak1
            | BlockId::GamePak2
            | BlockId::GbaSave
            | BlockId::NdsBios9
            | BlockId::NdsBios7
            | BlockId::NdsItcm
            | BlockId::NdsMainRam
            | BlockId::NdsDtcm
            | BlockId::NdsSharedWram
            | BlockId::NdsArm7Wram
            | BlockId::NdsIo
            | BlockId::NdsFifo
            | BlockId::NdsPalette
            | BlockId::NdsVram(_)
            | BlockId::NdsOam => {
                out.fill(0xFF);
                return;
            }
        };
        for (index, byte) in out.iter_mut().enumerate() {
            let offset = at.offset as usize + index;
            *byte = match bytes {
                Some(bytes) => bytes.get(offset).copied().unwrap_or(0xFF),
                None => self.peek_register(at.block, offset),
            };
        }
    }

    fn peek_register(&self, block: BlockId, offset: usize) -> u8 {
        let base = if block == BlockId::Io { 0xFF00 } else { 0xFF80 };
        if offset >= 0x80 {
            return 0xFF;
        }
        let addr = base + offset as u16;
        match addr {
            0xFF80..=0xFFFE => self.memory.hram()[offset],
            0xFFFF => self.ic.enable,
            _ => crate::gb::bus::read_io!(self, addr),
        }
    }

    pub fn poke(&mut self, at: Address, bytes: &[u8]) {
        let bank = usize::from(at.bank);
        let target = match at.block {
            BlockId::Vram => self.ppu.vram_mut(bank),
            BlockId::CartRam => self.cartridge.ram_bank_mut(bank),
            BlockId::Wram => self.memory.wram_bank_mut(bank),
            BlockId::Oam => Some(self.ppu.oam_mut()),
            BlockId::Hram => Some(self.memory.hram_mut()),
            BlockId::Io => None,
            BlockId::BootRom
            | BlockId::Rom
            | BlockId::Bios
            | BlockId::Ewram
            | BlockId::Iwram
            | BlockId::GbaIo
            | BlockId::Palette
            | BlockId::GbaVram
            | BlockId::GbaOam
            | BlockId::GamePak
            | BlockId::GamePak1
            | BlockId::GamePak2
            | BlockId::GbaSave
            | BlockId::NdsBios9
            | BlockId::NdsBios7
            | BlockId::NdsItcm
            | BlockId::NdsMainRam
            | BlockId::NdsDtcm
            | BlockId::NdsSharedWram
            | BlockId::NdsArm7Wram
            | BlockId::NdsIo
            | BlockId::NdsFifo
            | BlockId::NdsPalette
            | BlockId::NdsVram(_)
            | BlockId::NdsOam => return,
        };
        match target {
            Some(target) => {
                for (index, &byte) in bytes.iter().enumerate() {
                    let offset = at.offset as usize + index;
                    if let Some(slot) = target.get_mut(offset) {
                        *slot = byte;
                    } else if at.block == BlockId::Hram && offset == 0x7F {
                        self.ic.enable = byte;
                        return;
                    }
                }
            }
            None => {
                for (index, &byte) in bytes.iter().enumerate() {
                    let addr = at.cpu_address().wrapping_add(index as u16);
                    if (0xFF00..=0xFF7F).contains(&addr) {
                        self.write_io(addr, byte);
                    }
                }
            }
        }
    }
}

#[cfg(all(test, feature = "debug"))]
mod tests {
    use crate::ReadMemory;
    use crate::WriteMemory;
    use crate::debug::Debug;
    use crate::debug::breakpoints::{Access, Watch};
    use crate::debug::code::{Address, BlockId};
    use crate::debug::memory::MemoryBlock;
    use crate::debug::step::Stop;
    use crate::gb::{GameBoy, GbModel};
    use crate::rom::Rom;

    const TOBU: &[u8] = include_bytes!("../../../citrine-gb-app/homebrew/tobu.gb");

    const UCITY: &[u8] = include_bytes!("../../../citrine-gb-app/homebrew/ucity.gbc");

    fn after_frames(data: &[u8], model: GbModel, frames: usize) -> GameBoy {
        let mut gb = GameBoy::new_empty(model);
        gb.load_rom_with_model(&Rom::new(data), model)
            .expect("load");
        for _ in 0..frames {
            gb.run_frame();
        }
        gb
    }

    fn blocks(gb: &GameBoy) -> Vec<MemoryBlock> {
        let mut out = Vec::new();
        gb.memory_blocks(&mut out);
        out
    }

    fn block<'a>(blocks: &'a [MemoryBlock], name: &str) -> &'a MemoryBlock {
        blocks
            .iter()
            .find(|block| block.name == name)
            .unwrap_or_else(|| panic!("no block {name}"))
    }

    fn assert_peek_matches_the_bus(gb: &mut GameBoy) {
        gb.with_bus(|bus| bus.write_naive(0x0000, 0x0A));
        let cart_ram_mapped = gb.address_of(0xA000).is_some();
        for block in blocks(gb) {
            let mut checked = 0;
            for offset in 0..block.size {
                let cpu = (block.start + offset) as u16;
                let Some(at) = gb.address_of(cpu) else {
                    continue;
                };
                if at.block != block.id || !block.holds(at.bank) {
                    continue;
                }
                let mut peeked = [0];
                gb.peek(at, &mut peeked);
                let read = gb.with_bus(|bus| bus.read_naive(cpu));
                assert_eq!(peeked[0], read, "{} at {at}", block.name);
                checked += 1;
            }
            if block.id != BlockId::CartRam || cart_ram_mapped {
                assert!(checked > 0, "{} was never mapped", block.name);
            }
        }
    }

    #[test]
    fn peek_equals_bus_reads_in_every_block_of_a_game_boy() {
        let mut gb = after_frames(TOBU, GbModel::Dmg, 120);
        let names: Vec<_> = blocks(&gb).iter().map(|block| block.name).collect();
        assert_eq!(
            names,
            [
                "ROM bank 0",
                "Switchable ROM",
                "VRAM",
                "Cart RAM",
                "WRAM bank 0",
                "WRAM bank 1",
                "OAM",
                "I/O",
                "HRAM"
            ]
        );
        assert_peek_matches_the_bus(&mut gb);
    }

    #[test]
    fn peek_equals_bus_reads_in_every_bank_of_a_game_boy_color() {
        let mut gb = after_frames(UCITY, GbModel::Cgb, 120);
        let all = blocks(&gb);
        assert_eq!(block(&all, "VRAM").banks, 2);
        let wram = block(&all, "Switchable WRAM");
        assert_eq!((wram.first_bank, wram.banks), (1, 7));
        assert!(block(&all, "Cart RAM").writable);
        assert!(!block(&all, "Switchable ROM").writable);
        for (vbk, svbk) in [(0, 1), (1, 3), (0, 7)] {
            gb.with_bus(|bus| {
                bus.write_naive(0xFF4F, vbk);
                bus.write_naive(0xFF70, svbk);
            });
            assert_peek_matches_the_bus(&mut gb);
        }
    }

    #[test]
    fn the_boot_rom_is_a_block_only_while_it_is_mounted() {
        let mut boot = vec![0u8; 0x100];
        boot[0..2].copy_from_slice(&[0x18, 0xFE]);
        let mut gb = GameBoy::new(GbModel::Dmg, Some(boot), 0);
        let all = blocks(&gb);
        assert_eq!(block(&all, "Boot ROM").size, 0x100);
        assert_peek_matches_the_bus(&mut gb);

        gb.boot_rom.mounted = false;
        assert!(blocks(&gb).iter().all(|block| block.id != BlockId::BootRom));
    }

    #[test]
    fn peek_reads_any_bank_and_poke_writes_it() {
        let mut gb = after_frames(UCITY, GbModel::Cgb, 30);
        let at = Address::wram(3, 0x10);
        gb.poke(at, &[0xAB, 0xCD]);

        let mut peeked = [0; 2];
        gb.peek(at, &mut peeked);
        assert_eq!(peeked, [0xAB, 0xCD]);

        gb.with_bus(|bus| bus.write_naive(0xFF70, 3));
        assert_eq!(gb.with_bus(|bus| bus.read_naive(0xD011)), 0xCD);
    }

    #[test]
    fn peek_past_the_end_of_a_block_reads_open_bus() {
        let gb = after_frames(TOBU, GbModel::Dmg, 1);
        let mut peeked = [0; 4];
        gb.peek(Address::hram(0x7E), &mut peeked);
        assert_eq!(&peeked[2..], [0xFF, 0xFF]);
    }

    fn rom_with(code: &[u8]) -> Rom {
        let mut data = vec![0u8; 0x8000];
        data[0x0100..0x0104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]);
        data[0x0150..0x0150 + code.len()].copy_from_slice(code);
        Rom::new(&data)
    }

    fn console(code: &[u8]) -> GameBoy {
        let mut gb = GameBoy::new_empty(GbModel::Dmg);
        gb.load_rom(&rom_with(code)).expect("load");
        gb.poke(Address::wram(0, 0x123), &[0]);
        gb
    }

    const WRITES_TWICE: [u8; 10] = [0x3E, 0x42, 0xEA, 0x23, 0xC1, 0xEA, 0x23, 0xC1, 0x18, 0xFE];

    fn watch(gb: &mut GameBoy, access: Access) {
        gb.breakpoints().watch(Watch {
            from: Address::wram(0, 0x120),
            len: 8,
            access,
        });
    }

    #[test]
    fn a_write_watchpoint_on_a_wram_byte_stops_at_the_writing_instruction() {
        let mut gb = console(&WRITES_TWICE);
        watch(&mut gb, Access::Write);

        let stop = gb.run_for(100_000);

        assert_eq!(
            stop,
            Stop::Watchpoint {
                at: Address::wram(0, 0x123),
                access: Access::Write,
                by: Some(Address::rom(0, 0x152)),
            }
        );
        assert_eq!(gb.cpu.pc.wrapping_sub(1), 0x155);
        assert_eq!(gb.with_bus(|bus| bus.read_naive(0xC123)), 0x42);
    }

    #[test]
    fn a_watchpoint_stops_a_running_frame() {
        let mut gb = console(&WRITES_TWICE);
        watch(&mut gb, Access::Write);

        gb.run_frame();

        assert!(gb.debugger.hit_breakpoint);
        assert_eq!(gb.cpu.pc.wrapping_sub(1), 0x155);
        assert!(matches!(
            gb.debugger.breakpoints.last_stop(),
            Some(Stop::Watchpoint { .. })
        ));
    }

    #[test]
    fn a_change_watchpoint_ignores_a_write_of_the_same_value() {
        let mut gb = console(&WRITES_TWICE);
        watch(&mut gb, Access::Change);

        assert!(matches!(gb.run_for(100_000), Stop::Watchpoint { .. }));
        assert_eq!(gb.run_for(100_000), Stop::Finished);
    }

    #[test]
    fn a_read_watchpoint_stops_on_a_load_and_not_on_a_store() {
        let mut gb = console(&[0xEA, 0x23, 0xC1, 0x00, 0xFA, 0x23, 0xC1, 0x18, 0xFE]);
        watch(&mut gb, Access::Read);

        let stop = gb.run_for(100_000);

        assert_eq!(
            stop,
            Stop::Watchpoint {
                at: Address::wram(0, 0x123),
                access: Access::Read,
                by: Some(Address::rom(0, 0x154)),
            }
        );
    }

    #[test]
    fn a_removed_watchpoint_no_longer_stops() {
        let mut gb = console(&WRITES_TWICE);
        watch(&mut gb, Access::Write);
        let set = gb.breakpoints().watches()[0];
        gb.breakpoints().unwatch(set);

        assert_eq!(gb.run_for(100_000), Stop::Finished);
        assert!(gb.breakpoints().is_empty());
    }
}

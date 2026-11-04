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
            | BlockId::GbaSave => {
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
            | BlockId::GbaSave => return,
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

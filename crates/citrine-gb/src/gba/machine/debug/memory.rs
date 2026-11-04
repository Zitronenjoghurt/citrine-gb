use crate::debug::code::{Address, BlockId, GBA_BANK};
use crate::debug::memory::MemoryBlock;
use crate::gba::machine::Gba;
use crate::gba::region::{
    BIOS_SIZE, EWRAM_SIZE, IO_SIZE, IWRAM_SIZE, OAM_SIZE, PALETTE_SIZE, Region, VRAM_SIZE,
};

const PLAIN: [(BlockId, &str, usize); 7] = [
    (BlockId::Bios, "BIOS", BIOS_SIZE),
    (BlockId::Ewram, "EWRAM", EWRAM_SIZE),
    (BlockId::Iwram, "IWRAM", IWRAM_SIZE),
    (BlockId::GbaIo, "I/O", IO_SIZE),
    (BlockId::Palette, "Palette", PALETTE_SIZE),
    (BlockId::GbaVram, "VRAM", VRAM_SIZE),
    (BlockId::GbaOam, "OAM", OAM_SIZE),
];

const ROMS: [(BlockId, &str); 3] = [
    (BlockId::GamePak, "ROM"),
    (BlockId::GamePak1, "ROM wait state 1"),
    (BlockId::GamePak2, "ROM wait state 2"),
];

fn banked(id: BlockId, name: &'static str, len: usize) -> MemoryBlock {
    let start = Address::gba(id, 0).gba_address().unwrap_or(0);
    MemoryBlock {
        id,
        name,
        start,
        size: (len as u32).min(GBA_BANK),
        first_bank: 0,
        banks: len.div_ceil(GBA_BANK as usize) as u16,
        writable: false,
    }
}

impl Gba {
    pub(super) fn gba_memory_blocks(&self, out: &mut Vec<MemoryBlock>) {
        out.clear();
        for (id, name, size) in PLAIN {
            out.push(MemoryBlock {
                id,
                name,
                start: Address::gba(id, 0).gba_address().unwrap_or(0),
                size: size as u32,
                first_bank: 0,
                banks: 1,
                writable: id != BlockId::Bios,
            });
        }
        let rom_len = self.bus.rom_len();
        if rom_len > 0 {
            out.extend(ROMS.map(|(id, name)| banked(id, name, rom_len)));
        }
        let save = self.bus.backup().bytes().len();
        if save > 0 {
            out.push(banked(BlockId::GbaSave, "Save", save));
        }
    }

    pub(super) fn gba_locate(&self, addr: u32) -> Option<Address> {
        let region = Region::of(addr);
        let offset = region.offset(addr) as u32;
        let id = match region {
            Region::Bios => BlockId::Bios,
            Region::Ewram => BlockId::Ewram,
            Region::Iwram => BlockId::Iwram,
            Region::Io => BlockId::GbaIo,
            Region::Palette => BlockId::Palette,
            Region::Vram => BlockId::GbaVram,
            Region::Oam => BlockId::GbaOam,
            Region::Rom(_) if (offset as usize) < self.bus.rom_len() => {
                ROMS[(addr >> 25) as usize - 4].0
            }
            Region::Sram if (offset as usize) < self.bus.backup().bytes().len() => BlockId::GbaSave,
            Region::Rom(_) | Region::Sram | Region::Unmapped => return None,
        };
        let banked = matches!(
            id,
            BlockId::GamePak | BlockId::GamePak1 | BlockId::GamePak2 | BlockId::GbaSave
        );
        Some(if banked {
            Address::at(id, (offset / GBA_BANK) as u16, offset % GBA_BANK)
        } else {
            Address::gba(id, offset)
        })
    }

    pub(super) fn gba_peek(&self, at: Address, out: &mut [u8]) {
        if at.block == BlockId::GbaSave {
            let bytes = self.bus.backup().bytes();
            let start = (u32::from(at.bank) * GBA_BANK + at.offset) as usize;
            for (index, byte) in out.iter_mut().enumerate() {
                *byte = bytes.get(start + index).copied().unwrap_or(0xFF);
            }
            return;
        }
        let Some(addr) = at.gba_address() else {
            out.fill(0xFF);
            return;
        };
        let region = Region::of(addr);
        let start = region.offset(addr);
        if let Some(bytes) = self
            .bus
            .memory(region)
            .and_then(|memory| memory.get(start..start + out.len()))
        {
            out.copy_from_slice(bytes);
            return;
        }
        for (index, byte) in out.iter_mut().enumerate() {
            *byte = self.bus.peek8(addr.wrapping_add(index as u32));
        }
    }

    pub(super) fn gba_poke(&mut self, at: Address, bytes: &[u8]) {
        if at.block == BlockId::GbaSave {
            return;
        }
        let Some(addr) = at.gba_address() else {
            return;
        };
        for (index, &byte) in bytes.iter().enumerate() {
            self.bus.poke8(addr.wrapping_add(index as u32), byte);
        }
    }
}

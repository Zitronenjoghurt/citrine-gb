pub const BIOS_SIZE: usize = 0x4000;
pub const EWRAM_SIZE: usize = 0x4_0000;
pub const IWRAM_SIZE: usize = 0x8000;
pub const IO_SIZE: usize = 0x400;
pub const PALETTE_SIZE: usize = 0x400;
pub const VRAM_SIZE: usize = 0x1_8000;
pub const OAM_SIZE: usize = 0x400;
pub const ROM_MAX_SIZE: usize = 0x200_0000;
pub const SRAM_SIZE: usize = 0x1_0000;

const VRAM_MIRROR: usize = 0x2_0000;
const VRAM_UPPER_BANK: usize = 0x8000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WaitState {
    Zero,
    One,
    Two,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Region {
    Bios,
    Ewram,
    Iwram,
    Io,
    Palette,
    Vram,
    Oam,
    Rom(WaitState),
    Sram,
    Unmapped,
}

impl Region {
    pub fn of(addr: u32) -> Self {
        let low = (addr & 0x00FF_FFFF) as usize;
        match addr >> 24 {
            0x00 if low < BIOS_SIZE => Self::Bios,
            0x02 => Self::Ewram,
            0x03 => Self::Iwram,
            0x04 if low < IO_SIZE => Self::Io,
            0x05 => Self::Palette,
            0x06 => Self::Vram,
            0x07 => Self::Oam,
            0x08 | 0x09 => Self::Rom(WaitState::Zero),
            0x0A | 0x0B => Self::Rom(WaitState::One),
            0x0C | 0x0D => Self::Rom(WaitState::Two),
            0x0E | 0x0F => Self::Sram,
            _ => Self::Unmapped,
        }
    }

    pub fn offset(self, addr: u32) -> usize {
        let addr = addr as usize;
        match self {
            Self::Bios => addr,
            Self::Ewram => addr % EWRAM_SIZE,
            Self::Iwram => addr % IWRAM_SIZE,
            Self::Io => addr & 0x00FF_FFFF,
            Self::Palette => addr % PALETTE_SIZE,
            Self::Vram => {
                let offset = addr % VRAM_MIRROR;
                if offset >= VRAM_SIZE {
                    offset - VRAM_UPPER_BANK
                } else {
                    offset
                }
            }
            Self::Oam => addr % OAM_SIZE,
            Self::Rom(_) => addr % ROM_MAX_SIZE,
            Self::Sram => addr % SRAM_SIZE,
            Self::Unmapped => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_region_starts_at_its_gbatek_address() {
        let cases = [
            (0x0000_0000, Region::Bios),
            (0x0000_3FFF, Region::Bios),
            (0x0200_0000, Region::Ewram),
            (0x0300_0000, Region::Iwram),
            (0x0400_0000, Region::Io),
            (0x0400_03FF, Region::Io),
            (0x0500_0000, Region::Palette),
            (0x0600_0000, Region::Vram),
            (0x0700_0000, Region::Oam),
            (0x0800_0000, Region::Rom(WaitState::Zero)),
            (0x09FF_FFFF, Region::Rom(WaitState::Zero)),
            (0x0A00_0000, Region::Rom(WaitState::One)),
            (0x0BFF_FFFF, Region::Rom(WaitState::One)),
            (0x0C00_0000, Region::Rom(WaitState::Two)),
            (0x0DFF_FFFF, Region::Rom(WaitState::Two)),
            (0x0E00_0000, Region::Sram),
            (0x0FFF_FFFF, Region::Sram),
        ];
        for (addr, region) in cases {
            assert_eq!(Region::of(addr), region, "{addr:08X}");
        }
    }

    #[test]
    fn gaps_are_unmapped() {
        for addr in [
            0x0000_4000,
            0x01FF_FFFF,
            0x0100_0000,
            0x0400_0400,
            0x04FF_FFFF,
            0x1000_0000,
            0xFFFF_FFFF,
        ] {
            assert_eq!(Region::of(addr), Region::Unmapped, "{addr:08X}");
        }
    }

    #[test]
    fn offsets_wrap_at_each_region_size() {
        let cases = [
            (0x0204_0010, 0x10),
            (0x02FF_FFFF, EWRAM_SIZE - 1),
            (0x0300_8004, 0x4),
            (0x03FF_FFFC, 0x7FFC),
            (0x0500_0400, 0),
            (0x05FF_FFFF, PALETTE_SIZE - 1),
            (0x0700_0402, 2),
            (0x0A00_0010, 0x10),
            (0x0DFF_FFFF, ROM_MAX_SIZE - 1),
            (0x0E01_0001, 1),
            (0x0F00_0000, 0),
        ];
        for (addr, offset) in cases {
            assert_eq!(Region::of(addr).offset(addr), offset, "{addr:08X}");
        }
    }

    #[test]
    fn vram_upper_32k_mirrors_the_obj_bank() {
        let vram = Region::Vram;
        assert_eq!(vram.offset(0x0601_7FFF), 0x1_7FFF);
        assert_eq!(vram.offset(0x0601_8000), 0x1_0000);
        assert_eq!(vram.offset(0x0601_FFFF), 0x1_7FFF);
        assert_eq!(vram.offset(0x0602_0000), 0);
        assert_eq!(vram.offset(0x0603_8004), 0x1_0004);
    }
}

use super::region::Region;

const FIRST_ACCESS_WAITS: [u32; 4] = [4, 3, 2, 8];
const WAITCNT_MASK: u16 = 0x5FFF;
const PREFETCH: u16 = 1 << 14;
const ROM_PAGE: u32 = 0x2_0000;
pub(crate) const EWRAM_CYCLES: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Width {
    Byte,
    Half,
    Word,
}

impl Width {
    pub fn bytes(self) -> usize {
        match self {
            Self::Byte => 1,
            Self::Half => 2,
            Self::Word => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u8)]
pub enum Access {
    NonSequential,
    Sequential,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(from = "WaitBits", into = "WaitBits"))]
pub struct WaitControl {
    bits: u16,
    sequential_halves: u32,
    first_halves: u32,
}

#[cfg(feature = "serde")]
#[derive(serde::Serialize, serde::Deserialize)]
struct WaitBits {
    bits: u16,
}

#[cfg(feature = "serde")]
impl From<WaitBits> for WaitControl {
    fn from(saved: WaitBits) -> Self {
        Self::from_bits(saved.bits)
    }
}

#[cfg(feature = "serde")]
impl From<WaitControl> for WaitBits {
    fn from(wait: WaitControl) -> Self {
        Self { bits: wait.bits }
    }
}

impl Default for WaitControl {
    fn default() -> Self {
        Self::from_bits(0)
    }
}

impl WaitControl {
    fn from_bits(bits: u16) -> Self {
        Self {
            bits,
            sequential_halves: (0..4).fold(0, |halves, state| {
                halves | sequential_half(bits, state) << (8 * state)
            }),
            first_halves: (0..3).fold(0, |halves, state| {
                halves | first_half(bits, state) << (8 * state)
            }),
        }
    }

    pub fn read(self) -> u16 {
        self.bits
    }

    pub fn write(&mut self, value: u16, mask: u16) {
        let mask = mask & WAITCNT_MASK;
        *self = Self::from_bits((self.bits & !mask) | (value & mask));
    }

    pub fn prefetch(self) -> bool {
        self.bits & PREFETCH != 0
    }

    pub fn sram_waits(self) -> u32 {
        FIRST_ACCESS_WAITS[usize::from(self.bits & 0b11)]
    }

    #[inline(always)]
    pub fn rom_sequential_half(self, addr: u32) -> u32 {
        self.sequential_halves >> (addr >> 22 & 0x18) & 0xFF
    }

    #[inline(always)]
    fn rom_first_half(self, addr: u32) -> u32 {
        self.first_halves >> (addr >> 22 & 0x18) & 0xFF
    }

    #[inline(always)]
    pub fn cycles(self, addr: u32, width: Width, access: Access) -> u32 {
        self.region_cycles(Region::of(addr), addr, width, access)
    }

    pub fn region_cycles(self, region: Region, addr: u32, width: Width, access: Access) -> u32 {
        let halves = if width == Width::Word { 2 } else { 1 };
        match region {
            Region::Ewram => EWRAM_CYCLES * halves,
            Region::Palette | Region::Vram => halves,
            Region::Rom(_) => self.rom_cycles(addr, width, access),
            Region::Sram => 1 + self.sram_waits(),
            Region::Bios | Region::Iwram | Region::Io | Region::Oam | Region::Unmapped => 1,
        }
    }

    pub fn rom_cycles(self, addr: u32, width: Width, access: Access) -> u32 {
        let second = self.rom_sequential_half(addr);
        let first = if access == Access::Sequential && !addr.is_multiple_of(ROM_PAGE) {
            second
        } else {
            self.rom_first_half(addr)
        };
        if width == Width::Word {
            first + second
        } else {
            first
        }
    }
}

fn first_half(bits: u16, state: u32) -> u32 {
    1 + FIRST_ACCESS_WAITS[usize::from(bits >> (2 + 3 * state) & 0b11)]
}

fn sequential_half(bits: u16, state: u32) -> u32 {
    if bits & (1 << (4 + 3 * state)) != 0 {
        2
    } else {
        1 + (2 << state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gba::region::WaitState;
    use Access::{NonSequential, Sequential};
    use Width::{Byte, Half, Word};

    const ROM_ADDRS: [(u32, WaitState); 9] = [
        (0x0800_0002, WaitState::Zero),
        (0x0802_0000, WaitState::Zero),
        (0x09FF_FFFE, WaitState::Zero),
        (0x0A00_0000, WaitState::One),
        (0x0A00_1000, WaitState::One),
        (0x0BFF_0002, WaitState::One),
        (0x0C00_0002, WaitState::Two),
        (0x0C04_0000, WaitState::Two),
        (0x0DFF_FFFE, WaitState::Two),
    ];

    fn widths(wait: WaitControl, addr: u32, access: Access) -> [u32; 3] {
        [Byte, Half, Word].map(|width| wait.cycles(addr, width, access))
    }

    fn first_waits(wait: WaitControl, state: WaitState) -> u32 {
        let shift = 2 + 3 * state as u16;
        FIRST_ACCESS_WAITS[usize::from((wait.bits >> shift) & 0b11)]
    }

    fn second_waits(wait: WaitControl, state: WaitState) -> u32 {
        let fast = wait.bits & (1 << (4 + 3 * state as u16)) != 0;
        match (fast, state) {
            (true, _) => 1,
            (false, WaitState::Zero) => 2,
            (false, WaitState::One) => 4,
            (false, WaitState::Two) => 8,
        }
    }

    #[test]
    fn default_table_matches_the_memory_map() {
        let wait = WaitControl::default();
        let cases = [
            (0x0000_0000, [1, 1, 1]),
            (0x0200_0000, [3, 3, 6]),
            (0x0300_0000, [1, 1, 1]),
            (0x0400_0000, [1, 1, 1]),
            (0x0500_0000, [1, 1, 2]),
            (0x0600_0000, [1, 1, 2]),
            (0x0700_0000, [1, 1, 1]),
            (0x0800_0000, [5, 5, 8]),
            (0x0A00_0000, [5, 5, 10]),
            (0x0C00_0000, [5, 5, 14]),
            (0x0E00_0000, [5, 5, 5]),
            (0x1000_0000, [1, 1, 1]),
        ];
        for (addr, cycles) in cases {
            assert_eq!(widths(wait, addr, NonSequential), cycles, "{addr:08X}");
        }
    }

    #[test]
    fn sequential_rom_uses_the_second_access_waits() {
        let wait = WaitControl::default();
        assert_eq!(widths(wait, 0x0800_0002, Sequential), [3, 3, 6]);
        assert_eq!(widths(wait, 0x0A00_0002, Sequential), [5, 5, 10]);
        assert_eq!(widths(wait, 0x0C00_0002, Sequential), [9, 9, 18]);
    }

    #[test]
    fn sequential_access_is_non_sequential_at_a_128k_page() {
        let wait = WaitControl::default();
        assert_eq!(wait.cycles(0x0802_0000, Half, Sequential), 5);
        assert_eq!(wait.cycles(0x0802_0000, Word, Sequential), 8);
        assert_eq!(wait.cycles(0x0802_0002, Half, Sequential), 3);
    }

    #[test]
    fn sequential_only_matters_for_rom() {
        let wait = WaitControl::default();
        for addr in [0x0200_0000, 0x0500_0000, 0x0E00_0000] {
            assert_eq!(
                widths(wait, addr, Sequential),
                widths(wait, addr, NonSequential)
            );
        }
    }

    #[test]
    fn typical_cart_setting_4317() {
        let mut wait = WaitControl::default();
        wait.write(0x4317, 0xFFFF);
        assert_eq!(wait.sram_waits(), 8);
        assert_eq!(wait.cycles(0x0E00_0000, Byte, NonSequential), 9);
        assert_eq!(widths(wait, 0x0800_0000, NonSequential), [4, 4, 6]);
        assert_eq!(widths(wait, 0x0800_0002, Sequential), [2, 2, 4]);
        assert_eq!(widths(wait, 0x0A00_0000, NonSequential), [5, 5, 10]);
        assert_eq!(widths(wait, 0x0C00_0000, NonSequential), [9, 9, 18]);
        assert!(wait.prefetch());
    }

    #[test]
    fn every_first_access_setting() {
        let mut wait = WaitControl::default();
        for (setting, waits) in FIRST_ACCESS_WAITS.into_iter().enumerate() {
            let setting = setting as u16;
            wait.write(setting | setting << 2 | setting << 5 | setting << 8, 0xFFFF);
            assert_eq!(wait.sram_waits(), waits);
            for state in [WaitState::Zero, WaitState::One, WaitState::Two] {
                assert_eq!(first_waits(wait, state), waits, "{state:?}");
            }
        }
    }

    #[test]
    fn each_second_access_bit_sets_one_wait() {
        let mut wait = WaitControl::default();
        for (bit, state) in [
            (4, WaitState::Zero),
            (7, WaitState::One),
            (10, WaitState::Two),
        ] {
            wait.write(1 << bit, 0xFFFF);
            assert_eq!(second_waits(wait, state), 1, "{state:?}");
        }
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn a_loaded_wait_control_keeps_its_rom_costs() {
        for bits in [0, 0x4317, 0x5FFF] {
            let mut wait = WaitControl::default();
            wait.write(bits, 0xFFFF);
            let saved = rmp_serde::to_vec(&wait).unwrap();
            assert_eq!(rmp_serde::from_slice::<WaitControl>(&saved).unwrap(), wait);
        }
    }

    #[test]
    fn a_sequential_rom_half_costs_one_plus_its_second_waits() {
        for bits in 0..=WAITCNT_MASK {
            let mut wait = WaitControl::default();
            wait.write(bits, 0xFFFF);
            for (addr, state) in [
                (0x0800_0002, WaitState::Zero),
                (0x09FF_FFFE, WaitState::Zero),
                (0x0A00_1000, WaitState::One),
                (0x0BFF_0002, WaitState::One),
                (0x0C00_0002, WaitState::Two),
                (0x0DFF_FFFE, WaitState::Two),
            ] {
                assert_eq!(
                    wait.rom_sequential_half(addr),
                    1 + second_waits(wait, state),
                    "{bits:04X} {addr:08X}"
                );
            }
        }
    }

    #[test]
    fn rom_cycles_follow_the_waitcnt_fields_for_every_setting() {
        for bits in 0..=WAITCNT_MASK {
            let mut wait = WaitControl::default();
            wait.write(bits, 0xFFFF);
            for (addr, state) in ROM_ADDRS {
                for access in [NonSequential, Sequential] {
                    let second = second_waits(wait, state);
                    let first = if access == Sequential && !addr.is_multiple_of(ROM_PAGE) {
                        second
                    } else {
                        first_waits(wait, state)
                    };
                    assert_eq!(
                        widths(wait, addr, access),
                        [1 + first, 1 + first, 2 + first + second],
                        "{bits:04X} {addr:08X} {access:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn prefetch_bit_leaves_the_cycles_alone() {
        let mut wait = WaitControl::default();
        let before = widths(wait, 0x0800_0000, NonSequential);
        wait.write(PREFETCH, 0xFFFF);
        assert!(wait.prefetch());
        assert_eq!(widths(wait, 0x0800_0000, NonSequential), before);
    }

    #[test]
    fn read_back_drops_unused_and_cart_type_bits() {
        let mut wait = WaitControl::default();
        wait.write(0xFFFF, 0xFFFF);
        assert_eq!(wait.read(), 0x5FFF);
    }

    #[test]
    fn masked_write_keeps_the_other_byte() {
        let mut wait = WaitControl::default();
        wait.write(0x4317, 0xFFFF);
        wait.write(0x0000, 0x00FF);
        assert_eq!(wait.read(), 0x4300);
    }
}

use super::{Bank, Purpose};

const ENABLE: u8 = 0x80;
const OFS_SHIFT: u8 = 3;
const SLOT_128K: usize = 0x2_0000;
const SLOT_16K: usize = 0x4000;
const SLOT_8K: usize = 0x2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Mapping {
    pub purpose: Purpose,
    pub offset: usize,
    pub len: usize,
}

impl Mapping {
    pub fn decode(bank: Bank, control: u8) -> Option<Self> {
        if control & ENABLE == 0 {
            return None;
        }
        let mst = control & bank.mst_mask();
        let ofs = usize::from(control >> OFS_SHIFT & 3);
        let low = ofs & 1;
        let high = ofs >> 1;
        let small = low * SLOT_16K + high * 0x1_0000;
        let size = bank.size();
        let (purpose, offset, len) = match (bank, mst) {
            (_, 0) => (Purpose::Lcdc, bank.lcdc_offset(), size),
            (Bank::A | Bank::B | Bank::C | Bank::D, 1) => (Purpose::BgA, ofs * SLOT_128K, size),
            (Bank::A | Bank::B, 2) => (Purpose::ObjA, low * SLOT_128K, size),
            (Bank::C | Bank::D, 2) => (Purpose::Arm7, low * SLOT_128K, size),
            (Bank::A | Bank::B | Bank::C | Bank::D, 3) => (Purpose::Texture, ofs * SLOT_128K, size),
            (Bank::C, 4) => (Purpose::BgB, 0, size),
            (Bank::D, 4) => (Purpose::ObjB, 0, size),
            (Bank::E, 1) => (Purpose::BgA, 0, size),
            (Bank::E, 2) => (Purpose::ObjA, 0, size),
            (Bank::E, 3) => (Purpose::TexturePalette, 0, size),
            (Bank::E, 4) => (Purpose::BgExtPaletteA, 0, 4 * SLOT_8K),
            (Bank::F | Bank::G, 1) => (Purpose::BgA, small, size),
            (Bank::F | Bank::G, 2) => (Purpose::ObjA, small, size),
            (Bank::F | Bank::G, 3) => (Purpose::TexturePalette, (low + high * 4) * SLOT_16K, size),
            (Bank::F | Bank::G, 4) => (Purpose::BgExtPaletteA, low * 2 * SLOT_8K, size),
            (Bank::F | Bank::G, 5) => (Purpose::ObjExtPaletteA, 0, SLOT_8K),
            (Bank::H, 1) => (Purpose::BgB, 0, size),
            (Bank::H, 2) => (Purpose::BgExtPaletteB, 0, size),
            (Bank::I, 1) => (Purpose::BgB, 0x8000, size),
            (Bank::I, 2) => (Purpose::ObjB, 0, size),
            (Bank::I, 3) => (Purpose::ObjExtPaletteB, 0, SLOT_8K),
            _ => return None,
        };
        Some(Self {
            purpose,
            offset,
            len,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(bank: Bank, mst: u8, ofs: u8) -> Option<(Purpose, usize, usize)> {
        Mapping::decode(bank, ENABLE | ofs << OFS_SHIFT | mst).map(|m| (m.purpose, m.offset, m.len))
    }

    #[test]
    fn a_disabled_bank_maps_nowhere() {
        for bank in Bank::ALL {
            assert_eq!(Mapping::decode(bank, 0x00), None);
            assert_eq!(Mapping::decode(bank, 0x19), None);
        }
    }

    #[test]
    fn mst_0_puts_each_bank_at_its_lcdc_address() {
        let table = [
            (Bank::A, 0x680_0000, 0x2_0000),
            (Bank::B, 0x682_0000, 0x2_0000),
            (Bank::C, 0x684_0000, 0x2_0000),
            (Bank::D, 0x686_0000, 0x2_0000),
            (Bank::E, 0x688_0000, 0x1_0000),
            (Bank::F, 0x689_0000, 0x4000),
            (Bank::G, 0x689_4000, 0x4000),
            (Bank::H, 0x689_8000, 0x8000),
            (Bank::I, 0x68A_0000, 0x4000),
        ];
        for (bank, address, size) in table {
            assert_eq!(
                map(bank, 0, 3),
                Some((Purpose::Lcdc, address - 0x680_0000, size))
            );
        }
    }

    #[test]
    fn banks_a_to_d_in_mst_1_take_a_128k_engine_a_bg_slot_per_ofs() {
        for bank in [Bank::A, Bank::B, Bank::C, Bank::D] {
            assert_eq!(map(bank, 1, 0), Some((Purpose::BgA, 0x00000, 0x20000)));
            assert_eq!(map(bank, 1, 1), Some((Purpose::BgA, 0x20000, 0x20000)));
            assert_eq!(map(bank, 1, 2), Some((Purpose::BgA, 0x40000, 0x20000)));
            assert_eq!(map(bank, 1, 3), Some((Purpose::BgA, 0x60000, 0x20000)));
        }
    }

    #[test]
    fn banks_a_and_b_in_mst_2_use_only_ofs_bit_0_for_engine_a_obj() {
        for bank in [Bank::A, Bank::B] {
            assert_eq!(map(bank, 2, 0), Some((Purpose::ObjA, 0x00000, 0x20000)));
            assert_eq!(map(bank, 2, 1), Some((Purpose::ObjA, 0x20000, 0x20000)));
            assert_eq!(map(bank, 2, 2), Some((Purpose::ObjA, 0x00000, 0x20000)));
        }
    }

    #[test]
    fn banks_c_and_d_in_mst_2_become_arm7_work_ram() {
        for bank in [Bank::C, Bank::D] {
            assert_eq!(map(bank, 2, 0), Some((Purpose::Arm7, 0x00000, 0x20000)));
            assert_eq!(map(bank, 2, 1), Some((Purpose::Arm7, 0x20000, 0x20000)));
        }
    }

    #[test]
    fn banks_a_to_d_in_mst_3_take_the_texture_slot_ofs() {
        for bank in [Bank::A, Bank::B, Bank::C, Bank::D] {
            for ofs in 0..4u8 {
                let slot = usize::from(ofs) * 0x20000;
                assert_eq!(map(bank, 3, ofs), Some((Purpose::Texture, slot, 0x20000)));
            }
        }
    }

    #[test]
    fn banks_c_and_d_in_mst_4_go_to_engine_b() {
        assert_eq!(map(Bank::C, 4, 2), Some((Purpose::BgB, 0, 0x20000)));
        assert_eq!(map(Bank::D, 4, 2), Some((Purpose::ObjB, 0, 0x20000)));
    }

    #[test]
    fn banks_a_and_b_ignore_mst_bit_2() {
        assert_eq!(map(Bank::A, 4, 0), Some((Purpose::Lcdc, 0, 0x20000)));
        assert_eq!(map(Bank::B, 5, 1), Some((Purpose::BgA, 0x20000, 0x20000)));
    }

    #[test]
    fn bank_e_ignores_ofs_in_every_mode() {
        for ofs in 0..4u8 {
            assert_eq!(map(Bank::E, 1, ofs), Some((Purpose::BgA, 0, 0x10000)));
            assert_eq!(map(Bank::E, 2, ofs), Some((Purpose::ObjA, 0, 0x10000)));
            assert_eq!(
                map(Bank::E, 3, ofs),
                Some((Purpose::TexturePalette, 0, 0x10000))
            );
            assert_eq!(
                map(Bank::E, 4, ofs),
                Some((Purpose::BgExtPaletteA, 0, 0x8000))
            );
        }
    }

    #[test]
    fn banks_f_and_g_in_mst_1_and_2_use_both_ofs_bits() {
        let starts = [0x0000, 0x4000, 0x10000, 0x14000];
        for bank in [Bank::F, Bank::G] {
            for (ofs, start) in (0..4u8).zip(starts) {
                assert_eq!(map(bank, 1, ofs), Some((Purpose::BgA, start, 0x4000)));
                assert_eq!(map(bank, 2, ofs), Some((Purpose::ObjA, start, 0x4000)));
            }
        }
    }

    #[test]
    fn banks_f_and_g_in_mst_3_take_texture_palette_slots_0_1_4_5() {
        let slots = [0, 1, 4, 5];
        for bank in [Bank::F, Bank::G] {
            for (ofs, slot) in (0..4u8).zip(slots) {
                let start = slot * 0x4000;
                assert_eq!(
                    map(bank, 3, ofs),
                    Some((Purpose::TexturePalette, start, 0x4000))
                );
            }
        }
    }

    #[test]
    fn banks_f_and_g_in_mst_4_take_bg_ext_palette_slots_0_1_or_2_3() {
        for bank in [Bank::F, Bank::G] {
            assert_eq!(
                map(bank, 4, 0),
                Some((Purpose::BgExtPaletteA, 0x0000, 0x4000))
            );
            assert_eq!(
                map(bank, 4, 1),
                Some((Purpose::BgExtPaletteA, 0x4000, 0x4000))
            );
        }
    }

    #[test]
    fn banks_f_and_g_in_mst_5_give_their_lower_8k_to_obj_ext_palette() {
        for bank in [Bank::F, Bank::G] {
            assert_eq!(map(bank, 5, 2), Some((Purpose::ObjExtPaletteA, 0, 0x2000)));
        }
    }

    #[test]
    fn bank_h_maps_engine_b_bg_and_its_ext_palettes() {
        assert_eq!(map(Bank::H, 1, 3), Some((Purpose::BgB, 0, 0x8000)));
        assert_eq!(
            map(Bank::H, 2, 3),
            Some((Purpose::BgExtPaletteB, 0, 0x8000))
        );
        assert_eq!(map(Bank::H, 3, 0), None);
    }

    #[test]
    fn bank_i_maps_engine_b_bg_obj_and_obj_ext_palette() {
        assert_eq!(map(Bank::I, 1, 3), Some((Purpose::BgB, 0x8000, 0x4000)));
        assert_eq!(map(Bank::I, 2, 3), Some((Purpose::ObjB, 0, 0x4000)));
        assert_eq!(
            map(Bank::I, 3, 3),
            Some((Purpose::ObjExtPaletteB, 0, 0x2000))
        );
    }

    #[test]
    fn unused_mst_values_map_nowhere() {
        for mst in 5..8 {
            assert_eq!(map(Bank::C, mst, 0), None);
            assert_eq!(map(Bank::D, mst, 0), None);
            assert_eq!(map(Bank::E, mst, 0), None);
        }
        for mst in 6..8 {
            assert_eq!(map(Bank::F, mst, 0), None);
            assert_eq!(map(Bank::G, mst, 0), None);
        }
    }
}

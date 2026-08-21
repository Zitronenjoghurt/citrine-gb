use crate::gb::GbModel;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RamInit {
    #[default]
    Zeroed,
    Random {
        seed: u64,
    },
}

impl RamInit {
    pub const DEFAULT_SEED: u64 = 0x0C17_A17E;

    pub fn random() -> Self {
        Self::Random {
            seed: Self::DEFAULT_SEED,
        }
    }

    pub(crate) fn rng(&self) -> Option<RamRng> {
        match self {
            Self::Zeroed => None,
            Self::Random { seed } => Some(RamRng(*seed)),
        }
    }
}

pub(crate) struct RamRng(u64);

impl RamRng {
    fn next_byte(&mut self) -> u8 {
        self.0 = self
            .0
            .wrapping_mul(0x27BB_2EE6_87B0_B0FD)
            .wrapping_add(0xB504_F32D);
        (self.0 >> 56) as u8
    }

    pub(crate) fn wram_byte(&mut self, index: usize, model: GbModel) -> u8 {
        let byte = self.next_byte();
        match model {
            GbModel::Dmg => {
                if index & 0x100 != 0 {
                    byte & self.next_byte()
                } else {
                    byte | self.next_byte()
                }
            }
            GbModel::Cgb => byte,
        }
    }

    pub(crate) fn hram_byte(&mut self, index: usize, model: GbModel) -> u8 {
        match model {
            GbModel::Dmg => {
                if index & 1 != 0 {
                    self.next_byte() | self.next_byte() | self.next_byte()
                } else {
                    self.next_byte() & self.next_byte() & self.next_byte()
                }
            }
            GbModel::Cgb => self.next_byte(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gb::GameBoy;
    use crate::{ReadMemory, WriteMemory};

    fn wram(gb: &GameBoy) -> Vec<u8> {
        (0xC000..=0xDFFF).map(|a| gb.memory.read_naive(a)).collect()
    }

    #[test]
    fn zeroed_is_the_default() {
        let gb = GameBoy::new_empty(GbModel::Dmg);
        assert_eq!(gb.ram_init, RamInit::Zeroed);
        assert!(wram(&gb).iter().all(|&b| b == 0));
    }

    #[test]
    fn random_is_reproducible_for_a_seed() {
        let a = GameBoy::new_empty_with_ram_init(GbModel::Dmg, RamInit::Random { seed: 42 });
        let b = GameBoy::new_empty_with_ram_init(GbModel::Dmg, RamInit::Random { seed: 42 });
        let c = GameBoy::new_empty_with_ram_init(GbModel::Dmg, RamInit::Random { seed: 43 });
        assert_eq!(wram(&a), wram(&b));
        assert_ne!(wram(&a), wram(&c));
        assert!(wram(&a).iter().any(|&b| b != 0), "should not be all zeroes");
    }

    #[test]
    fn dmg_wram_alternates_bit_bias_every_256_bytes() {
        let gb = GameBoy::new_empty_with_ram_init(GbModel::Dmg, RamInit::random());
        let ram = wram(&gb);
        let ones =
            |range: std::ops::Range<usize>| -> u32 { range.map(|i| ram[i].count_ones()).sum() };
        assert!(
            ones(0..0x100) > ones(0x100..0x200),
            "0x000 block should be denser than 0x100 block"
        );
        assert!(
            ones(0x200..0x300) > ones(0x300..0x400),
            "0x200 block should be denser than 0x300 block"
        );
    }

    #[test]
    fn ram_init_survives_a_rom_load() {
        let mut gb = GameBoy::new_empty_with_ram_init(GbModel::Dmg, RamInit::random());
        gb.memory.write_naive(0xC000, 0x00);
        let rom = crate::rom::Rom::new(&vec![0u8; 0x8000]);
        gb.load_rom(&rom).expect("load");
        assert_eq!(gb.ram_init, RamInit::random());
        assert!(
            wram(&gb).iter().any(|&b| b != 0),
            "reload should refill garbage, not zeroes"
        );
    }
}

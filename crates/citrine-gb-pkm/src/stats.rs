#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Stats<T> {
    pub hp: T,
    pub atk: T,
    pub def: T,
    pub spe: T,
    pub spa: T,
    pub spd: T,
}

impl<T: Copy> Stats<T> {
    pub fn splat(value: T) -> Self {
        Self {
            hp: value,
            atk: value,
            def: value,
            spe: value,
            spa: value,
            spd: value,
        }
    }

    pub fn to_array(self) -> [T; 6] {
        [self.hp, self.atk, self.def, self.spe, self.spa, self.spd]
    }

    pub fn from_array([hp, atk, def, spe, spa, spd]: [T; 6]) -> Self {
        Self {
            hp,
            atk,
            def,
            spe,
            spa,
            spd,
        }
    }
}

impl Stats<u8> {
    pub(crate) fn pack_ivs(self) -> u32 {
        self.to_array()
            .iter()
            .enumerate()
            .fold(0, |packed, (i, &iv)| {
                packed | (u32::from(iv & 0x1F) << (5 * i))
            })
    }

    pub(crate) fn unpack_ivs(packed: u32) -> Self {
        Self::from_array(std::array::from_fn(|i| ((packed >> (5 * i)) & 0x1F) as u8))
    }

    pub fn total(self) -> u32 {
        self.to_array().iter().map(|&v| u32::from(v)).sum()
    }
}

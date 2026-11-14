mod gen4;
mod gen5;

pub use gen4::{
    Calls, Frame, Lead, LeadOutcome, Method, Spread, WildRoll, cute_charm_pid, generate, seeds,
    wild, wild_rolls,
};
pub use gen5::{
    Gen5Wild, MersenneTwister, Rng64, gen5_ability_slot, gen5_ivs, gen5_wild, gen5_wild_pid,
    gen5_wild_pid_fits,
};

pub(crate) use crate::pkm::nature_of;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lcrng(pub u32);

impl Lcrng {
    const MUL: u32 = 0x41C6_4E6D;
    const ADD: u32 = 0x6073;
    const MUL_BACK: u32 = 0xEEB9_EB65;
    const ADD_BACK: u32 = 0x0A35_61A1;

    pub fn next_u16(&mut self) -> u16 {
        self.0 = self.0.wrapping_mul(Self::MUL).wrapping_add(Self::ADD);
        (self.0 >> 16) as u16
    }

    pub fn prev_u16(&mut self) -> u16 {
        let out = (self.0 >> 16) as u16;
        self.0 = self
            .0
            .wrapping_mul(Self::MUL_BACK)
            .wrapping_add(Self::ADD_BACK);
        out
    }

    pub(crate) fn forward(state: u32) -> u32 {
        state.wrapping_mul(Self::MUL).wrapping_add(Self::ADD)
    }

    pub(crate) fn back(state: u32) -> u32 {
        state
            .wrapping_mul(Self::MUL_BACK)
            .wrapping_add(Self::ADD_BACK)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prev_undoes_next() {
        let mut rng = Lcrng(0x1A56_B091);
        for _ in 0..100 {
            rng.next_u16();
        }
        for _ in 0..100 {
            rng.prev_u16();
        }
        assert_eq!(rng, Lcrng(0x1A56_B091));
    }

    #[test]
    fn matches_smogons_published_outputs() {
        let mut rng = Lcrng(0x1A56_B091);
        let outs: Vec<u16> = (0..6).map(|_| rng.next_u16()).collect();
        assert_eq!(outs, [0x01DB, 0x7B06, 0x5233, 0xE470, 0x5CC4, 0x36BB]);
    }
}

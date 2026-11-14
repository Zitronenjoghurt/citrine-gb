use crate::stats::Stats;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rng64(pub u64);

impl Rng64 {
    const MUL: u64 = 0x5D58_8B65_6C07_8965;
    const ADD: u64 = 0x0026_9EC3;
    const MUL_BACK: u64 = 0xDEDC_EDAE_9638_806D;
    const ADD_BACK: u64 = 0x9B1A_E6E9_A384_E6F9;

    pub fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(Self::MUL).wrapping_add(Self::ADD);
        (self.0 >> 32) as u32
    }

    pub fn prev_u32(&mut self) -> u32 {
        let out = (self.0 >> 32) as u32;
        self.0 = self
            .0
            .wrapping_mul(Self::MUL_BACK)
            .wrapping_add(Self::ADD_BACK);
        out
    }

    pub fn below(&mut self, max: u32) -> u32 {
        ((u64::from(self.next_u32()) * u64::from(max)) >> 32) as u32
    }
}

pub struct MersenneTwister {
    state: [u32; 624],
    index: usize,
}

impl MersenneTwister {
    pub fn new(seed: u32) -> Self {
        let mut state = [0u32; 624];
        state[0] = seed;
        for i in 1..624 {
            let prev = state[i - 1];
            state[i] = 1_812_433_253u32
                .wrapping_mul(prev ^ (prev >> 30))
                .wrapping_add(i as u32);
        }
        Self { state, index: 624 }
    }

    pub fn next_u32(&mut self) -> u32 {
        if self.index >= 624 {
            self.twist();
        }
        let mut y = self.state[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9D2C_5680;
        y ^= (y << 15) & 0xEFC6_0000;
        y ^ (y >> 18)
    }

    fn twist(&mut self) {
        for i in 0..624 {
            let y = (self.state[i] & 0x8000_0000) | (self.state[(i + 1) % 624] & 0x7FFF_FFFF);
            let mut next = self.state[(i + 397) % 624] ^ (y >> 1);
            if y & 1 != 0 {
                next ^= 0x9908_B0DF;
            }
            self.state[i] = next;
        }
        self.index = 0;
    }
}

pub fn gen5_ivs(mt_seed: u32, frame: u32) -> Stats<u8> {
    let mut mt = MersenneTwister::new(mt_seed);
    for _ in 1..frame {
        mt.next_u32();
    }
    let [hp, atk, def, spa, spd, spe] = std::array::from_fn(|_| (mt.next_u32() >> 27) as u8);
    Stats {
        hp,
        atk,
        def,
        spe,
        spa,
        spd,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Gen5Wild {
    pub pid: u32,
    pub nature: u8,
}

pub fn gen5_wild(seed: u64, tid: u16, sid: u16) -> Gen5Wild {
    let mut rng = Rng64(seed);
    let pid = gen5_wild_pid(rng.next_u32(), tid, sid);
    let nature = rng.below(25) as u8;
    Gen5Wild { pid, nature }
}

pub fn gen5_wild_pid(raw: u32, tid: u16, sid: u16) -> u32 {
    let pid = raw ^ 0x0001_0000;
    if (pid >> 31) ^ (pid & 1) != u32::from((tid ^ sid) & 1) {
        pid ^ 0x8000_0000
    } else {
        pid
    }
}

pub fn gen5_wild_pid_fits(pid: u32, tid: u16, sid: u16) -> bool {
    (pid >> 31) ^ (pid & 1) == u32::from((tid ^ sid) & 1)
}

pub fn gen5_ability_slot(pid: u32) -> u8 {
    ((pid >> 16) & 1) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mersenne_twister_matches_the_reference_outputs() {
        let mut mt = MersenneTwister::new(5489);
        assert_eq!(mt.next_u32(), 3_499_211_612);
        assert_eq!(mt.next_u32(), 581_869_302);
        assert_eq!(mt.next_u32(), 3_890_346_734);
    }

    #[test]
    fn rng64_prev_undoes_next() {
        let mut rng = Rng64(0x0123_4567_89AB_CDEF);
        let outs: Vec<u32> = (0..50).map(|_| rng.next_u32()).collect();
        let back: Vec<u32> = (0..50).map(|_| rng.prev_u32()).collect();
        assert_eq!(rng, Rng64(0x0123_4567_89AB_CDEF));
        assert_eq!(outs.into_iter().rev().collect::<Vec<_>>(), back);
    }

    #[test]
    fn rng64_from_zero_gives_the_increment_first() {
        let mut rng = Rng64(0);
        rng.next_u32();
        assert_eq!(rng.0, 0x0026_9EC3);
    }

    #[test]
    fn wild_pids_follow_the_trainer_parity_rule() {
        for (raw, tid, sid) in [
            (0u32, 0u16, 0u16),
            (1, 0, 0),
            (0x8000_0001, 1, 0),
            (0x1234_5678, 12345, 54321),
        ] {
            let pid = gen5_wild_pid(raw, tid, sid);
            assert!(gen5_wild_pid_fits(pid, tid, sid));
            assert_eq!(pid & 0x7FFF_FFFF, (raw ^ 0x0001_0000) & 0x7FFF_FFFF);
        }
    }

    #[test]
    fn wild_ability_comes_from_bit_16() {
        assert_eq!(gen5_ability_slot(gen5_wild_pid(0, 0, 0)), 1);
        assert_eq!(gen5_ability_slot(gen5_wild_pid(0x0001_0000, 0, 0)), 0);
    }

    #[test]
    fn natures_and_ivs_stay_in_range() {
        for seed in 0..200u64 {
            let wild = gen5_wild(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15), 1, 2);
            assert!(wild.nature < 25);
        }
        let ivs = gen5_ivs(0x1234_5678, 1);
        assert!(ivs.to_array().iter().all(|&iv| iv < 32));
        assert_ne!(gen5_ivs(0x1234_5678, 1), gen5_ivs(0x1234_5678, 2));
    }
}

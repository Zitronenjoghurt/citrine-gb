use super::psr::{FLAG_C, FLAG_N, FLAG_V, FLAG_Z};

#[derive(Debug, Clone, Copy)]
pub struct Flags {
    pub n: bool,
    pub z: bool,
    pub c: bool,
    pub v: bool,
}

impl Flags {
    pub fn of(result: u32, c: bool, v: bool) -> Self {
        Self {
            n: result >> 31 != 0,
            z: result == 0,
            c,
            v,
        }
    }

    pub fn apply(self, cpsr: u32) -> u32 {
        let mut bits = cpsr & !(FLAG_N | FLAG_Z | FLAG_C | FLAG_V);
        for (set, flag) in [
            (self.n, FLAG_N),
            (self.z, FLAG_Z),
            (self.c, FLAG_C),
            (self.v, FLAG_V),
        ] {
            if set {
                bits |= flag;
            }
        }
        bits
    }
}

pub fn add(a: u32, b: u32, carry: bool) -> (u32, bool, bool) {
    let wide = u64::from(a) + u64::from(b) + u64::from(carry);
    let result = wide as u32;
    let overflow = (!(a ^ b) & (a ^ result)) >> 31 != 0;
    (result, wide > 0xFFFF_FFFF, overflow)
}

pub fn sub(a: u32, b: u32, carry: bool) -> (u32, bool, bool) {
    let (result, c, _) = add(a, !b, carry);
    let overflow = ((a ^ b) & (a ^ result)) >> 31 != 0;
    (result, c, overflow)
}

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
    let (sum, c1) = a.overflowing_add(b);
    let (result, c2) = sum.overflowing_add(u32::from(carry));
    let (signed, v1) = (a as i32).overflowing_add(b as i32);
    let v2 = signed.overflowing_add(i32::from(carry)).1;
    (result, c1 | c2, v1 ^ v2)
}

pub fn sub(a: u32, b: u32, carry: bool) -> (u32, bool, bool) {
    let (difference, b1) = a.overflowing_sub(b);
    let (result, b2) = difference.overflowing_sub(u32::from(!carry));
    let (signed, v1) = (a as i32).overflowing_sub(b as i32);
    let v2 = signed.overflowing_sub(i32::from(!carry)).1;
    (result, !(b1 | b2), v1 ^ v2)
}

#[cfg(test)]
mod tests {
    use super::{add, sub};

    const VALUES: [u32; 12] = [
        0,
        1,
        2,
        0x7FFF_FFFE,
        0x7FFF_FFFF,
        0x8000_0000,
        0x8000_0001,
        0xFFFF_FFFE,
        0xFFFF_FFFF,
        0x1234_5678,
        0x9ABC_DEF0,
        0x5555_AAAA,
    ];

    fn wide_add(a: u32, b: u32, carry: bool) -> (u32, bool, bool) {
        let unsigned = u64::from(a) + u64::from(b) + u64::from(carry);
        let signed = i64::from(a as i32) + i64::from(b as i32) + i64::from(carry);
        (
            unsigned as u32,
            unsigned > 0xFFFF_FFFF,
            signed != i64::from(signed as i32),
        )
    }

    #[test]
    fn add_and_sub_match_wide_arithmetic() {
        for a in VALUES {
            for b in VALUES {
                for carry in [false, true] {
                    assert_eq!(add(a, b, carry), wide_add(a, b, carry), "{a:#x} + {b:#x}");
                    assert_eq!(sub(a, b, carry), wide_add(a, !b, carry), "{a:#x} - {b:#x}");
                }
            }
        }
    }
}

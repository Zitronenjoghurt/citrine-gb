#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    Short,
    LongSigned,
    LongUnsigned,
}

impl Flavor {
    fn signed(self) -> bool {
        matches!(self, Self::Short | Self::LongSigned)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Csa {
    output: u64,
    carry: u64,
}

#[derive(Debug, Clone, Copy, Default)]
struct U128 {
    lo: u64,
    hi: u64,
}

impl U128 {
    fn ror(self, shift: u32) -> Self {
        Self {
            lo: (self.lo >> shift) | (self.hi << (64 - shift)),
            hi: (self.hi >> shift) | (self.lo << (64 - shift)),
        }
    }
}

fn mask(lo: u32, hi: u32) -> u64 {
    ((1u64 << (hi - lo)) - 1) << lo
}

fn bit(value: u64, n: u32) -> u64 {
    (value >> n) & 1
}

fn sign_extend(value: u64, from: u32, to: u32) -> u64 {
    if bit(value, from - 1) != 0 {
        value | mask(from, to)
    } else {
        value
    }
}

fn asr(value: u64, shift: u32, size: u32) -> u64 {
    let extended = sign_extend(value, size, 64) as i64;
    ((extended >> shift) as u64) & mask(0, size)
}

fn recode(input: u64, chunk: u64) -> (u64, u64) {
    let (value, carry) = match chunk {
        1 | 2 => (input, 0),
        3 => (input.wrapping_mul(2), 0),
        4 => (!input.wrapping_mul(2), 1),
        5 | 6 => (!input, 1),
        _ => (0, 0),
    };
    (value & 0x3_FFFF_FFFF, carry)
}

fn csa(a: u64, b: u64, c: u64) -> Csa {
    Csa {
        output: a ^ b ^ c,
        carry: (a & b) | (b & c) | (c & a),
    }
}

fn csa_array(previous: Csa, addends: [(u64, u64); 4], high_acc: &mut u64) -> Csa {
    let mut current = previous;
    let mut done = Csa::default();
    for (i, (addend, addend_carry)) in addends.into_iter().enumerate() {
        current.output &= 0x1_FFFF_FFFF;
        current.carry &= 0x1_FFFF_FFFF;

        let mut result = csa(current.output, addend & 0x1_FFFF_FFFF, current.carry);
        result.carry = (result.carry << 1) | addend_carry;

        done.output |= (result.output & 3) << (2 * i);
        done.carry |= (result.carry & 3) << (2 * i);

        result.output >>= 2;
        result.carry >>= 2;

        let magic = bit(*high_acc, 0) + (bit(current.carry, 32) ^ 1) + (bit(addend, 33) ^ 1);
        result.output |= magic << 31;
        result.carry |= (bit(*high_acc, 1) ^ 1) << 32;
        *high_acc >>= 2;

        current = result;
    }
    done.output |= current.output << 8;
    done.carry |= current.carry << 8;
    done
}

pub fn carry(flavor: Flavor, multiplicand: u32, multiplier: u32, accumulator: u64) -> bool {
    let (mut multiplier, multiplicand) = if flavor.signed() {
        (
            sign_extend(u64::from(multiplier), 32, 34),
            sign_extend(u64::from(multiplicand), 32, 34),
        )
    } else {
        (u64::from(multiplier), u64::from(multiplicand))
    };

    let mut state = Csa {
        output: accumulator,
        carry: if multiplier & 1 != 0 {
            !multiplicand
        } else {
            0
        },
    };
    let mut high_acc = accumulator >> 34;

    let mut partial_carry = U128 {
        lo: state.carry & 1,
        hi: 0,
    };
    state.output >>= 1;
    state.carry >>= 1;
    partial_carry = partial_carry.ror(1);

    let mut iterations = 0;
    loop {
        let addends: [(u64, u64); 4] =
            std::array::from_fn(|i| recode(multiplicand, (multiplier >> (2 * i)) & 0b111));
        state = csa_array(state, addends, &mut high_acc);

        partial_carry.lo |= state.carry & 0xFF;
        state.output >>= 8;
        state.carry >>= 8;
        partial_carry = partial_carry.ror(8);

        multiplier = asr(multiplier, 8, 33);
        iterations += 1;
        let done = multiplier == 0 || (flavor.signed() && multiplier == 0x1_FFFF_FFFF);
        if done {
            break;
        }
    }
    partial_carry.lo |= state.carry;

    let correction = match iterations {
        1 => 23,
        2 => 15,
        3 => 7,
        _ => 31,
    };
    partial_carry = partial_carry.ror(correction);

    if flavor == Flavor::Short && iterations == 4 {
        (partial_carry.hi >> 31) & 1 != 0
    } else {
        (partial_carry.hi >> 63) & 1 != 0
    }
}

#[cfg(test)]
mod tests;

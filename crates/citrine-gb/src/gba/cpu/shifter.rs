#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shift {
    Lsl,
    Lsr,
    Asr,
    Ror,
}

impl Shift {
    pub fn from_bits(bits: u32) -> Self {
        match bits & 3 {
            0 => Self::Lsl,
            1 => Self::Lsr,
            2 => Self::Asr,
            _ => Self::Ror,
        }
    }

    pub fn by_immediate(self, value: u32, amount: u32, carry: bool) -> (u32, bool) {
        match (self, amount) {
            (Self::Lsl, 0) => (value, carry),
            (Self::Lsr | Self::Asr, 0) => self.by_register(value, 32, carry),
            (Self::Ror, 0) => ((u32::from(carry) << 31) | (value >> 1), value & 1 != 0),
            _ => self.by_register(value, amount, carry),
        }
    }

    pub fn by_register(self, value: u32, amount: u32, carry: bool) -> (u32, bool) {
        if amount == 0 {
            return (value, carry);
        }
        match self {
            Self::Lsl => match amount {
                1..=31 => (value << amount, (value >> (32 - amount)) & 1 != 0),
                32 => (0, value & 1 != 0),
                _ => (0, false),
            },
            Self::Lsr => match amount {
                1..=31 => (value >> amount, (value >> (amount - 1)) & 1 != 0),
                32 => (0, value >> 31 != 0),
                _ => (0, false),
            },
            Self::Asr => match amount {
                1..=31 => (
                    ((value as i32) >> amount) as u32,
                    (value >> (amount - 1)) & 1 != 0,
                ),
                _ => (((value as i32) >> 31) as u32, value >> 31 != 0),
            },
            Self::Ror => {
                let result = value.rotate_right(amount & 31);
                (result, result >> 31 != 0)
            }
        }
    }
}

pub fn rotated_immediate(op: u32, carry: bool) -> (u32, bool) {
    let rotate = ((op >> 8) & 0xF) * 2;
    let value = (op & 0xFF).rotate_right(rotate);
    if rotate == 0 {
        (value, carry)
    } else {
        (value, value >> 31 != 0)
    }
}

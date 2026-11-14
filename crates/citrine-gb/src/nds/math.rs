pub const FIRST: usize = 0x280;
pub const LAST: usize = 0x2BF;

const DIVCNT: usize = 0x00;
const DIV_NUMER: usize = 0x10;
const DIV_NUMER_HIGH: usize = 0x14;
const DIV_DENOM: usize = 0x18;
const DIV_DENOM_HIGH: usize = 0x1C;
const DIV_RESULT: usize = 0x20;
const DIV_RESULT_HIGH: usize = 0x24;
const DIVREM_RESULT: usize = 0x28;
const DIVREM_RESULT_HIGH: usize = 0x2C;
const SQRTCNT: usize = 0x30;
const SQRT_RESULT: usize = 0x34;
const SQRT_PARAM: usize = 0x38;
const SQRT_PARAM_HIGH: usize = 0x3C;

const DIV_MODE: u32 = 0x3;
const SQRT_MODE: u32 = 0x1;
const DIV_BY_ZERO: u32 = 1 << 14;
const BUSY: u32 = 1 << 15;
const DIV_CYCLES_32: u32 = 18;
const DIV_CYCLES_64: u32 = 34;
const SQRT_CYCLES: u32 = 13;

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Math {
    div_mode: u32,
    numer: u64,
    denom: u64,
    quotient: u64,
    remainder: u64,
    div_busy: u32,
    sqrt_mode: u32,
    param: u64,
    root: u32,
    sqrt_busy: u32,
}

impl Math {
    pub fn read(&self, offset: usize) -> u32 {
        match offset & !3 {
            DIVCNT => self.divcnt(),
            DIV_NUMER => low(self.numer),
            DIV_NUMER_HIGH => high(self.numer),
            DIV_DENOM => low(self.denom),
            DIV_DENOM_HIGH => high(self.denom),
            DIV_RESULT => low(self.quotient),
            DIV_RESULT_HIGH => high(self.quotient),
            DIVREM_RESULT => low(self.remainder),
            DIVREM_RESULT_HIGH => high(self.remainder),
            SQRTCNT => self.sqrtcnt(),
            SQRT_RESULT => self.root,
            SQRT_PARAM => low(self.param),
            SQRT_PARAM_HIGH => high(self.param),
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: usize, value: u32, mask: u32) {
        match offset & !3 {
            DIVCNT => {
                self.div_mode = merge(self.div_mode, value, mask & DIV_MODE);
                self.divide();
            }
            DIV_NUMER => {
                self.numer = merge_low(self.numer, value, mask);
                self.divide();
            }
            DIV_NUMER_HIGH => {
                self.numer = merge_high(self.numer, value, mask);
                self.divide();
            }
            DIV_DENOM => {
                self.denom = merge_low(self.denom, value, mask);
                self.divide();
            }
            DIV_DENOM_HIGH => {
                self.denom = merge_high(self.denom, value, mask);
                self.divide();
            }
            SQRTCNT => {
                self.sqrt_mode = merge(self.sqrt_mode, value, mask & SQRT_MODE);
                self.square_root();
            }
            SQRT_PARAM => {
                self.param = merge_low(self.param, value, mask);
                self.square_root();
            }
            SQRT_PARAM_HIGH => {
                self.param = merge_high(self.param, value, mask);
                self.square_root();
            }
            _ => {}
        }
    }

    pub fn step(&mut self, cycles: u32) {
        self.div_busy = self.div_busy.saturating_sub(cycles);
        self.sqrt_busy = self.sqrt_busy.saturating_sub(cycles);
    }

    pub fn busy(&self) -> bool {
        self.div_busy > 0 || self.sqrt_busy > 0
    }

    fn divcnt(&self) -> u32 {
        let by_zero = if self.denom == 0 { DIV_BY_ZERO } else { 0 };
        let busy = if self.div_busy > 0 { BUSY } else { 0 };
        self.div_mode | by_zero | busy
    }

    fn sqrtcnt(&self) -> u32 {
        let busy = if self.sqrt_busy > 0 { BUSY } else { 0 };
        self.sqrt_mode | busy
    }

    fn divide(&mut self) {
        let (quotient, remainder) = match self.div_mode {
            0 => divide_32(self.numer as i32, self.denom as i32),
            2 => divide_64(self.numer as i64, self.denom as i64),
            _ => divide_64(self.numer as i64, i64::from(self.denom as i32)),
        };
        self.quotient = quotient;
        self.remainder = remainder;
        self.div_busy = if self.div_mode == 0 {
            DIV_CYCLES_32
        } else {
            DIV_CYCLES_64
        };
    }

    fn square_root(&mut self) {
        let param = if self.sqrt_mode == 0 {
            self.param & 0xFFFF_FFFF
        } else {
            self.param
        };
        self.root = param.isqrt() as u32;
        self.sqrt_busy = SQRT_CYCLES;
    }
}

fn divide_32(numer: i32, denom: i32) -> (u64, u64) {
    let overflow = denom == 0 || (numer == i32::MIN && denom == -1);
    let (quotient, remainder) = if denom == 0 {
        (if numer < 0 { 1 } else { -1 }, numer)
    } else {
        (numer.wrapping_div(denom), numer.wrapping_rem(denom))
    };
    let quotient = i64::from(quotient) as u64;
    let quotient = if overflow {
        quotient ^ 0xFFFF_FFFF_0000_0000
    } else {
        quotient
    };
    (quotient, i64::from(remainder) as u64)
}

fn divide_64(numer: i64, denom: i64) -> (u64, u64) {
    if denom == 0 {
        let quotient: i64 = if numer < 0 { 1 } else { -1 };
        return (quotient as u64, numer as u64);
    }
    (
        numer.wrapping_div(denom) as u64,
        numer.wrapping_rem(denom) as u64,
    )
}

fn low(value: u64) -> u32 {
    value as u32
}

fn high(value: u64) -> u32 {
    (value >> 32) as u32
}

fn merge(old: u32, value: u32, mask: u32) -> u32 {
    (old & !mask) | (value & mask)
}

fn merge_low(old: u64, value: u32, mask: u32) -> u64 {
    (old & !0xFFFF_FFFF) | u64::from(merge(low(old), value, mask))
}

fn merge_high(old: u64, value: u32, mask: u32) -> u64 {
    (old & 0xFFFF_FFFF) | (u64::from(merge(high(old), value, mask)) << 32)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: u32 = 0xFFFF_FFFF;

    fn divide(mode: u32, numer: i64, denom: i64) -> Math {
        let mut math = Math::default();
        math.write(DIVCNT, mode, ALL);
        math.write(DIV_NUMER, numer as u32, ALL);
        math.write(DIV_NUMER_HIGH, (numer >> 32) as u32, ALL);
        math.write(DIV_DENOM, denom as u32, ALL);
        math.write(DIV_DENOM_HIGH, (denom >> 32) as u32, ALL);
        math
    }

    fn quotient(math: &Math) -> u64 {
        u64::from(math.read(DIV_RESULT)) | (u64::from(math.read(DIV_RESULT_HIGH)) << 32)
    }

    fn remainder(math: &Math) -> u64 {
        u64::from(math.read(DIVREM_RESULT)) | (u64::from(math.read(DIVREM_RESULT_HIGH)) << 32)
    }

    fn root(mode: u32, param: u64) -> Math {
        let mut math = Math::default();
        math.write(SQRTCNT, mode, ALL);
        math.write(SQRT_PARAM, param as u32, ALL);
        math.write(SQRT_PARAM_HIGH, (param >> 32) as u32, ALL);
        math
    }

    #[test]
    fn mode_0_divides_32_bit_and_sign_extends() {
        let math = divide(0, -7, 2);
        assert_eq!(quotient(&math), -3i64 as u64);
        assert_eq!(remainder(&math), -1i64 as u64);
    }

    #[test]
    fn mode_0_ignores_the_upper_halves() {
        let math = divide(0, 0x1234_5678_0000_0064, 0x7777_0000_0000_0007);
        assert_eq!(quotient(&math), 14);
        assert_eq!(remainder(&math), 2);
    }

    #[test]
    fn mode_1_divides_64_by_32() {
        let math = divide(1, 0x10_0000_0000, 0x7777_0000_0000_0010);
        assert_eq!(quotient(&math), 0x1_0000_0000);
        assert_eq!(remainder(&math), 0);
    }

    #[test]
    fn mode_1_sign_extends_the_denominator() {
        let math = divide(1, 100, 0xFFFF_FFFF);
        assert_eq!(quotient(&math), -100i64 as u64);
    }

    #[test]
    fn mode_2_divides_64_by_64() {
        let math = divide(2, -0x30_0000_0001, 0x10_0000_0000);
        assert_eq!(quotient(&math), -3i64 as u64);
        assert_eq!(remainder(&math), -1i64 as u64);
    }

    #[test]
    fn mode_3_behaves_like_mode_1() {
        let math = divide(3, 100, 0x1_FFFF_FFFF);
        assert_eq!(quotient(&math), -100i64 as u64);
        assert_eq!(math.read(DIVCNT) & DIV_MODE, 3);
    }

    #[test]
    fn mode_0_by_zero_gives_minus_one_with_inverted_upper_half() {
        let math = divide(0, 5, 0);
        assert_eq!(quotient(&math), 0x0000_0000_FFFF_FFFF);
        assert_eq!(remainder(&math), 5);
    }

    #[test]
    fn mode_0_negative_by_zero_gives_one_with_inverted_upper_half() {
        let math = divide(0, -5, 0);
        assert_eq!(quotient(&math), 0xFFFF_FFFF_0000_0001);
        assert_eq!(remainder(&math), -5i64 as u64);
    }

    #[test]
    fn mode_0_minus_max_by_minus_one_reads_plus_max_in_64_bits() {
        let math = divide(0, i64::from(i32::MIN), -1);
        assert_eq!(quotient(&math), 0x0000_0000_8000_0000);
        assert_eq!(remainder(&math), 0);
    }

    #[test]
    fn mode_2_by_zero_gives_opposite_sign_one() {
        let positive = divide(2, 0x1_0000_0000, 0);
        assert_eq!(quotient(&positive), u64::MAX);
        assert_eq!(remainder(&positive), 0x1_0000_0000);
        let negative = divide(2, -9, 0);
        assert_eq!(quotient(&negative), 1);
        assert_eq!(remainder(&negative), -9i64 as u64);
    }

    #[test]
    fn mode_0_zero_by_zero_counts_as_positive() {
        assert_eq!(quotient(&divide(0, 0, 0)), 0x0000_0000_FFFF_FFFF);
    }

    #[test]
    fn mode_2_minus_max_by_minus_one_stays_minus_max() {
        let math = divide(2, i64::MIN, -1);
        assert_eq!(quotient(&math), i64::MIN as u64);
        assert_eq!(remainder(&math), 0);
    }

    #[test]
    fn mode_1_by_zero_lower_half_gives_opposite_sign_one() {
        let math = divide(1, 7, 0x5_0000_0000);
        assert_eq!(quotient(&math), u64::MAX);
        assert_eq!(remainder(&math), 7);
    }

    #[test]
    fn div_by_zero_flag_checks_the_whole_denominator() {
        assert_ne!(divide(0, 1, 0).read(DIVCNT) & DIV_BY_ZERO, 0);
        assert_eq!(divide(0, 1, 0x1_0000_0000).read(DIVCNT) & DIV_BY_ZERO, 0);
        assert_eq!(
            divide(0, i64::from(i32::MIN), -1).read(DIVCNT) & DIV_BY_ZERO,
            0
        );
    }

    #[test]
    fn mode_0_by_zero_in_the_lower_half_still_divides_by_zero() {
        let math = divide(0, 5, 0x1_0000_0000);
        assert_eq!(quotient(&math), 0x0000_0000_FFFF_FFFF);
        assert_eq!(remainder(&math), 5);
    }

    #[test]
    fn division_is_busy_for_18_cycles_in_mode_0() {
        let mut math = divide(0, 10, 3);
        math.step(17);
        assert_ne!(math.read(DIVCNT) & BUSY, 0);
        math.step(1);
        assert_eq!(math.read(DIVCNT) & BUSY, 0);
    }

    #[test]
    fn division_is_busy_for_34_cycles_in_64_bit_modes() {
        for mode in 1..=3 {
            let mut math = divide(mode, 10, 3);
            math.step(33);
            assert_ne!(math.read(DIVCNT) & BUSY, 0, "mode {mode}");
            math.step(1);
            assert_eq!(math.read(DIVCNT) & BUSY, 0, "mode {mode}");
        }
    }

    #[test]
    fn each_operand_write_restarts_the_division() {
        let mut math = divide(0, 10, 3);
        math.step(18);
        math.write(DIV_DENOM, 5, 0xFFFF);
        assert_ne!(math.read(DIVCNT) & BUSY, 0);
        assert_eq!(quotient(&math), 2);
    }

    #[test]
    fn divcnt_keeps_only_the_mode_bits() {
        let mut math = Math::default();
        math.write(DIVCNT, 0xFFFF_3FFF, ALL);
        assert_eq!(math.read(DIVCNT) & !(BUSY | DIV_BY_ZERO), 3);
    }

    #[test]
    fn byte_writes_merge_into_the_operand() {
        let mut math = divide(0, 0x100, 1);
        math.write(DIV_NUMER, 0x0000_0012, 0x0000_00FF);
        assert_eq!(math.read(DIV_NUMER), 0x112);
        assert_eq!(quotient(&math), 0x112);
    }

    #[test]
    fn square_root_32_bit_uses_the_lower_half() {
        let math = root(0, 0x1_0000_0019);
        assert_eq!(math.read(SQRT_RESULT), 5);
    }

    #[test]
    fn square_root_rounds_down() {
        assert_eq!(root(0, 24).read(SQRT_RESULT), 4);
        assert_eq!(root(0, 0xFFFF_FFFF).read(SQRT_RESULT), 0xFFFF);
    }

    #[test]
    fn square_root_64_bit_uses_the_whole_param() {
        assert_eq!(root(1, 0x1_0000_0000).read(SQRT_RESULT), 0x1_0000);
        assert_eq!(root(1, u64::MAX).read(SQRT_RESULT), 0xFFFF_FFFF);
    }

    #[test]
    fn square_root_is_busy_for_13_cycles() {
        for mode in 0..=1 {
            let mut math = root(mode, 81);
            math.step(12);
            assert_ne!(math.read(SQRTCNT) & BUSY, 0);
            assert!(math.busy());
            math.step(1);
            assert_eq!(math.read(SQRTCNT), mode);
            assert!(!math.busy());
        }
    }

    #[test]
    fn results_are_read_only() {
        let mut math = divide(0, 10, 3);
        math.write(DIV_RESULT, 99, ALL);
        math.write(SQRT_RESULT, 99, ALL);
        assert_eq!(quotient(&math), 3);
        assert_eq!(math.read(SQRT_RESULT), 0);
    }
}

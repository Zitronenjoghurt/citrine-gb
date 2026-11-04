use super::*;

fn mul_zero_carry(x: u32) -> bool {
    for (bits, half) in [(8, 0xC0), (16, 0xC000), (24, 0xC0_0000)] {
        if x >> bits == u32::MAX >> bits {
            let low = x & ((1 << bits) - 1);
            return low < half && low & 0x5555_5555 != 0;
        }
    }
    x >> 30 == 2
}

#[test]
fn mul_by_zero_matches_the_hardware_rule_for_high_multipliers() {
    for i in 0..256 {
        let multiplier = u32::MAX - i;
        assert_eq!(
            carry(Flavor::Short, 0, multiplier, 0),
            mul_zero_carry(multiplier),
            "multiplier {multiplier:08x}"
        );
    }
}

#[test]
fn a_short_multiplier_with_full_iterations_takes_the_final_booth_carry() {
    assert!(carry(Flavor::Short, 0, 0x8000_0000, 0));
    assert!(!carry(Flavor::Short, 0, 0x4000_0000, 0));
}

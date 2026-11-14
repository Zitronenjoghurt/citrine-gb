pub const HIGH: i16 = 0x7FFF;
pub const LOW: i16 = -0x7FFF;
const STEPS: u8 = 8;
const SILENT_DUTY: u8 = 7;
const NOISE_SEED: u16 = 0x7FFF;
const NOISE_TAPS: u16 = 0x6000;

pub fn square(duty: u8, step: u8) -> i16 {
    let duty = duty & 7;
    if duty != SILENT_DUTY && step % STEPS >= SILENT_DUTY - duty {
        HIGH
    } else {
        LOW
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Noise {
    lfsr: u16,
}

impl Default for Noise {
    fn default() -> Self {
        Self { lfsr: NOISE_SEED }
    }
}

impl Noise {
    pub fn step(&mut self) -> i16 {
        let carry = self.lfsr & 1 != 0;
        self.lfsr >>= 1;
        if carry {
            self.lfsr ^= NOISE_TAPS;
            LOW
        } else {
            HIGH
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern(duty: u8) -> String {
        (0..8)
            .map(|step| if square(duty, step) == HIGH { 'H' } else { 'L' })
            .collect()
    }

    #[test]
    fn each_duty_starts_low_and_ends_high() {
        assert_eq!(pattern(0), "LLLLLLLH");
        assert_eq!(pattern(1), "LLLLLLHH");
        assert_eq!(pattern(2), "LLLLLHHH");
        assert_eq!(pattern(3), "LLLLHHHH");
        assert_eq!(pattern(4), "LLLHHHHH");
        assert_eq!(pattern(5), "LLHHHHHH");
        assert_eq!(pattern(6), "LHHHHHHH");
    }

    #[test]
    fn duty_7_stays_low() {
        assert_eq!(pattern(7), "LLLLLLLL");
    }

    #[test]
    fn the_square_repeats_every_8_steps() {
        assert_eq!(square(3, 12), square(3, 4));
    }

    #[test]
    fn noise_starts_with_its_known_sequence() {
        let mut noise = Noise::default();
        let out: String = (0..16)
            .map(|_| if noise.step() == HIGH { 'H' } else { 'L' })
            .collect();
        assert_eq!(out, "LLLLLLLLLLLLLLHH");
    }

    #[test]
    fn noise_repeats_after_32767_samples() {
        let mut noise = Noise::default();
        let mut period = 0;
        loop {
            noise.step();
            period += 1;
            if noise == Noise::default() {
                break;
            }
        }
        assert_eq!(period, 0x7FFF);
    }
}

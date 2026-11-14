use crate::tables::{SILENT_DB, attack_rate, fall_rate, sustain_level};

pub const FLOOR: i32 = SILENT_DB * 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Attack,
    Decay,
    Sustain,
    Release,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Envelope {
    level: i32,
    attack: i32,
    decay: i32,
    sustain: i32,
    release: i32,
    stage: Stage,
}

impl Envelope {
    pub fn new(attack: u8, decay: u8, sustain: u8, release: u8) -> Self {
        Self {
            level: FLOOR,
            attack: attack_rate(attack),
            decay: fall_rate(decay),
            sustain: sustain_level(sustain),
            release: fall_rate(release),
            stage: Stage::Attack,
        }
    }

    pub fn level(&self) -> i32 {
        self.level
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    pub fn release(&mut self) {
        if self.stage != Stage::Done {
            self.stage = Stage::Release;
        }
    }

    pub fn step(&mut self) {
        match self.stage {
            Stage::Attack => {
                self.level = self.attack * self.level / 255;
                if self.level == 0 {
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.level -= self.decay;
                if self.level <= self.sustain {
                    self.level = self.sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => {}
            Stage::Release => {
                self.level -= self.release;
                if self.level <= FLOOR {
                    self.level = FLOOR;
                    self.stage = Stage::Done;
                }
            }
            Stage::Done => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(envelope: &mut Envelope, steps: usize) -> Vec<i32> {
        (0..steps)
            .map(|_| {
                envelope.step();
                envelope.level()
            })
            .collect()
    }

    #[test]
    fn the_fastest_attack_jumps_to_full_and_decays_to_the_sustain_level() {
        let mut envelope = Envelope::new(127, 126, 120, 127);
        assert_eq!(run(&mut envelope, 3), [0, -1280, -1280]);
        assert_eq!(envelope.stage(), Stage::Sustain);
        envelope.release();
        assert_eq!(run(&mut envelope, 2), [-1280 - 0xFFFF, FLOOR]);
        assert_eq!(envelope.stage(), Stage::Done);
    }

    #[test]
    fn a_slow_attack_shrinks_the_attenuation_by_its_rate_each_step() {
        let mut envelope = Envelope::new(0x76, 127, 127, 127);
        assert_eq!(
            run(&mut envelope, 2),
            [-92544 * 0x54 / 255, (-92544 * 0x54 / 255) * 0x54 / 255]
        );
        assert_eq!(envelope.stage(), Stage::Attack);
    }

    #[test]
    fn release_falls_by_its_rate_until_silence() {
        let mut envelope = Envelope::new(127, 127, 127, 100);
        envelope.step();
        envelope.release();
        let rate = 0x1E00 / 26;
        assert_eq!(run(&mut envelope, 2), [-rate, -2 * rate]);
        let steps = run(&mut envelope, 1000);
        assert!(steps.contains(&FLOOR));
        assert_eq!(envelope.stage(), Stage::Done);
    }
}

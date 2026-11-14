use citrine_gb::console::{Console, Touch, VideoSpec};
use citrine_gb::debug::Debug;
use citrine_gb::machine::Machine;
use citrine_gb::persistence::state::StateBody;
use citrine_gb_report::FrameInput;
use std::collections::VecDeque;

pub(crate) const CHECKPOINT_FRAMES: usize = 600;
const SHOT_FRAMES: usize = 60;
const SHOTS: usize = 5;

pub(crate) struct Checkpoint {
    pub(crate) body: StateBody,
    pub(crate) now: u64,
    inputs: Vec<FrameInput>,
}

pub(crate) struct Shot {
    pub(crate) rgba: Vec<u8>,
    pub(crate) video: VideoSpec,
}

pub(crate) struct Recorded<'a> {
    pub(crate) checkpoint: &'a Checkpoint,
    pub(crate) inputs: Vec<FrameInput>,
}

#[derive(Default)]
pub(crate) struct Flight {
    older: Option<Checkpoint>,
    newer: Option<Checkpoint>,
    touch: Option<Touch>,
    reset_pending: bool,
    expected_cycles: Option<u128>,
    since_shot: usize,
    shots: VecDeque<Shot>,
}

impl Flight {
    pub(crate) fn set_touch(&mut self, touch: Option<Touch>) {
        self.touch = touch;
    }

    pub(crate) fn note_reset(&mut self) {
        self.reset_pending = true;
    }

    pub(crate) fn restart(&mut self) {
        self.older = None;
        self.newer = None;
        self.reset_pending = false;
        self.expected_cycles = None;
    }

    pub(crate) fn before_frame(&mut self, machine: &mut Machine, now: u64) {
        let cycles = Debug::cycles(machine);
        if self.expected_cycles != Some(cycles) && !self.reset_pending {
            self.restart();
        }
        let due = self
            .newer
            .as_ref()
            .is_none_or(|newer| newer.inputs.len() >= CHECKPOINT_FRAMES && !self.reset_pending);
        if due {
            if self.newer.is_none() {
                self.reset_pending = false;
            }
            match machine.state_body(now) {
                Ok(body) => {
                    self.older = self.newer.take();
                    self.newer = Some(Checkpoint {
                        body,
                        now,
                        inputs: Vec::with_capacity(CHECKPOINT_FRAMES),
                    });
                }
                Err(err) => {
                    log::warn!("game report: could not copy the state: {err:?}");
                    self.restart();
                    return;
                }
            }
        }
        if let Some(newer) = &mut self.newer {
            newer.inputs.push(FrameInput {
                keys: machine.keys(),
                touch: self.touch,
                reset: std::mem::take(&mut self.reset_pending),
            });
        }
    }

    pub(crate) fn shoots_next(&self) -> bool {
        self.since_shot + 1 >= SHOT_FRAMES || self.shots.is_empty()
    }

    pub(crate) fn after_frame(&mut self, machine: &mut Machine) {
        self.expected_cycles = Some(Debug::cycles(machine));
        let due = self.shoots_next();
        self.since_shot += 1;
        if !due {
            return;
        }
        self.since_shot = 0;
        machine.finish_picture();
        let mut rgba = if self.shots.len() == SHOTS {
            self.shots
                .pop_front()
                .map(|shot| shot.rgba)
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        rgba.clear();
        rgba.extend_from_slice(machine.frame());
        self.shots.push_back(Shot {
            rgba,
            video: machine.video(),
        });
    }

    pub(crate) fn recorded(&self) -> Option<Recorded<'_>> {
        let newer = self.newer.as_ref()?;
        Some(match &self.older {
            Some(older) => Recorded {
                checkpoint: older,
                inputs: [older.inputs.as_slice(), newer.inputs.as_slice()].concat(),
            },
            None => Recorded {
                checkpoint: newer,
                inputs: newer.inputs.clone(),
            },
        })
    }

    pub(crate) fn shots(&self) -> impl DoubleEndedIterator<Item = &Shot> {
        self.shots.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb::console::Keys;
    use citrine_gb::gb::{GameBoy, GbModel};
    use citrine_gb::rom::Rom;

    const TOBU: &[u8] = include_bytes!("../../homebrew/tobu.gb");

    fn machine() -> Machine {
        let mut gb = GameBoy::new_empty(GbModel::Dmg);
        gb.load_rom(&Rom::new(TOBU)).unwrap();
        Machine::Gb(Box::new(gb))
    }

    fn run(flight: &mut Flight, machine: &mut Machine, frames: usize) {
        for _ in 0..frames {
            flight.before_frame(machine, 1_790_000_000);
            machine.run_frame();
            flight.after_frame(machine);
        }
    }

    #[test]
    fn the_recording_reaches_back_one_to_two_checkpoints() {
        let mut flight = Flight::default();
        let mut machine = machine();
        run(&mut flight, &mut machine, 10);
        assert_eq!(flight.recorded().unwrap().inputs.len(), 10);
        run(&mut flight, &mut machine, CHECKPOINT_FRAMES);
        assert_eq!(
            flight.recorded().unwrap().inputs.len(),
            CHECKPOINT_FRAMES + 10
        );
        run(&mut flight, &mut machine, CHECKPOINT_FRAMES);
        assert_eq!(
            flight.recorded().unwrap().inputs.len(),
            CHECKPOINT_FRAMES + 10
        );
    }

    #[test]
    fn a_jump_in_the_clock_starts_the_recording_over() {
        let mut flight = Flight::default();
        let mut machine = machine();
        run(&mut flight, &mut machine, 30);
        machine.run_frame();
        run(&mut flight, &mut machine, 5);
        assert_eq!(flight.recorded().unwrap().inputs.len(), 5);
    }

    #[test]
    fn a_reset_is_kept_with_the_frame_after_it() {
        let mut flight = Flight::default();
        let mut machine = machine();
        run(&mut flight, &mut machine, 3);
        machine.soft_reset();
        flight.note_reset();
        machine.set_keys(Keys::START);
        run(&mut flight, &mut machine, 2);
        let inputs = flight.recorded().unwrap().inputs;
        assert_eq!(inputs.len(), 5);
        assert!(inputs[3].reset);
        assert_eq!(inputs[3].keys, Keys::START);
        assert!(!inputs[4].reset);
    }

    #[test]
    fn pictures_are_kept_a_second_apart_and_only_the_newest_five() {
        let mut flight = Flight::default();
        let mut machine = machine();
        run(&mut flight, &mut machine, SHOT_FRAMES * 8);
        assert_eq!(flight.shots().count(), SHOTS);
        assert_eq!(flight.shots().next().unwrap().rgba.len(), 160 * 144 * 4);
    }
}

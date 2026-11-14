use citrine_gb::console::Keys;
use citrine_gb::netplay::{CableEvent, Input};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const PAD: [Keys; 9] = [
    Keys::empty(),
    Keys::LEFT,
    Keys::RIGHT,
    Keys::UP,
    Keys::DOWN,
    Keys::A,
    Keys::B,
    Keys::A.union(Keys::RIGHT),
    Keys::L.union(Keys::R),
];

pub(crate) fn script(seed: u64, frames: usize) -> Vec<Input> {
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let mut inputs = Vec::with_capacity(frames);
    while inputs.len() < frames {
        let keys = if inputs.len() % 240 < 6 {
            Keys::START
        } else {
            PAD[rng.below(PAD.len())]
        };
        let length = 1 + rng.below(20);
        inputs.extend(std::iter::repeat_n(Input::new(keys), length));
    }
    inputs.truncate(frames);
    inputs
}

pub(crate) fn petris_link(seed: u64, frames: usize) -> [Vec<Input>; 2] {
    let mut inputs = [
        vec![Input::default(); frames],
        vec![Input::default(); frames],
    ];
    let mut press = |side: usize, at: usize, keys: Keys, length: usize| {
        for input in inputs[side].iter_mut().skip(at).take(length) {
            input.buttons = keys;
        }
    };
    press(0, 450, Keys::START, 5);
    press(1, 450, Keys::START, 5);
    press(0, 600, Keys::DOWN, 5);
    press(0, 640, Keys::RIGHT, 5);
    press(0, 700, Keys::START, 5);
    let pad = [Keys::LEFT, Keys::RIGHT, Keys::A, Keys::DOWN, Keys::empty()];
    let mut rng = Rng(seed | 1);
    for side in 0..2 {
        let mut at = 800;
        while at < frames {
            let length = (4 + rng.below(20)).min(frames - at);
            press(side, at, pad[rng.below(pad.len())], length);
            at += length;
        }
    }
    if let Some(first) = inputs[0].first_mut() {
        first.cable = Some(CableEvent::Plug);
    }
    inputs
}

pub(crate) fn pair(seed: u64, frames: usize, plugged: bool) -> [Vec<Input>; 2] {
    let mut inputs = [script(seed, frames), script(seed ^ 0x5555, frames)];
    if plugged && let Some(first) = inputs[0].first_mut() {
        first.cable = Some(CableEvent::Plug);
    }
    inputs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seed_always_gives_the_same_script() {
        assert_eq!(
            script(7, 500)
                .iter()
                .map(|i| i.buttons.bits())
                .collect::<Vec<_>>(),
            script(7, 500)
                .iter()
                .map(|i| i.buttons.bits())
                .collect::<Vec<_>>()
        );
        assert_ne!(
            script(7, 500)
                .iter()
                .map(|i| i.buttons.bits())
                .collect::<Vec<_>>(),
            script(8, 500)
                .iter()
                .map(|i| i.buttons.bits())
                .collect::<Vec<_>>()
        );
    }
}

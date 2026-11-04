use crate::pad_input::PadAxis;
use citrine_gb::console::Keys;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputSource {
    Keyboard,
    Touch,
    PadButtons(usize),
    PadAxis(usize, PadAxis),
}

impl InputSource {
    fn pad(self) -> Option<usize> {
        match self {
            Self::PadButtons(pad) | Self::PadAxis(pad, _) => Some(pad),
            Self::Keyboard | Self::Touch => None,
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Change {
    pub pressed: Keys,
    pub released: Keys,
}

#[derive(Debug, Default)]
pub struct HeldButtons {
    sources: Vec<(InputSource, Keys)>,
    held: Keys,
    fresh: Keys,
    owed: Keys,
}

impl HeldButtons {
    pub fn held(&self) -> Keys {
        self.held
    }

    pub fn of(&self, source: InputSource) -> Keys {
        self.sources
            .iter()
            .find(|(held_by, _)| *held_by == source)
            .map_or(Keys::empty(), |&(_, buttons)| buttons)
    }

    pub fn set(&mut self, source: InputSource, mask: Keys, down: Keys) -> Change {
        let next = (self.of(source) - mask) | (down & mask);
        self.sources.retain(|(held_by, _)| *held_by != source);
        if !next.is_empty() {
            self.sources.push((source, next));
        }
        self.settle()
    }

    pub fn forget_pad(&mut self, pad: usize) -> Change {
        self.sources
            .retain(|(held_by, _)| held_by.pad() != Some(pad));
        self.settle()
    }

    pub fn clear(&mut self) -> Change {
        self.sources.clear();
        let mut change = self.settle();
        change.released |= self.take_owed();
        change
    }

    pub fn take_owed(&mut self) -> Keys {
        self.fresh = Keys::empty();
        std::mem::take(&mut self.owed)
    }

    fn settle(&mut self) -> Change {
        let want = self
            .sources
            .iter()
            .fold(Keys::empty(), |all, &(_, buttons)| all | buttons);
        let before = self.held | self.owed;
        let let_go = self.held - want;
        self.fresh |= want - self.held;
        self.owed = (self.owed | (let_go & self.fresh)) - want;
        self.held = want;
        let after = self.held | self.owed;
        Change {
            pressed: after - before,
            released: before - after,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEFT: Keys = Keys::LEFT;
    const RIGHT: Keys = Keys::RIGHT;
    const STICK: InputSource = InputSource::PadAxis(0, PadAxis::LeftStickX);
    const BUTTONS: InputSource = InputSource::PadButtons(0);

    fn played(held: &mut HeldButtons) {
        held.take_owed();
    }

    #[test]
    fn a_button_stays_held_while_any_source_holds_it() {
        let mut held = HeldButtons::default();
        held.set(BUTTONS, LEFT, LEFT);
        held.set(InputSource::Keyboard, LEFT, LEFT);
        played(&mut held);
        let change = held.set(BUTTONS, LEFT, Keys::empty());
        assert!(change.released.is_empty());
        assert!(held.held().contains(LEFT));
        let change = held.set(InputSource::Keyboard, LEFT, Keys::empty());
        assert_eq!(change.released.bits(), LEFT.bits());
    }

    #[test]
    fn a_centred_stick_lets_go_of_its_own_buttons_only() {
        let mut held = HeldButtons::default();
        held.set(BUTTONS, LEFT, LEFT);
        held.set(STICK, LEFT | RIGHT, RIGHT);
        played(&mut held);
        let change = held.set(STICK, LEFT | RIGHT, Keys::empty());
        assert_eq!(change.released.bits(), RIGHT.bits());
        assert_eq!(held.held().bits(), LEFT.bits());
    }

    #[test]
    fn a_release_before_any_frame_waits_for_one() {
        let mut held = HeldButtons::default();
        let pressed = held.set(BUTTONS, LEFT, LEFT);
        assert_eq!(pressed.pressed.bits(), LEFT.bits());
        let change = held.set(BUTTONS, LEFT, Keys::empty());
        assert!(change.released.is_empty(), "the game has not seen it yet");
        assert!(held.held().is_empty());
        assert_eq!(held.take_owed().bits(), LEFT.bits());
        assert!(held.take_owed().is_empty());
    }

    #[test]
    fn a_press_again_before_the_frame_keeps_it_down() {
        let mut held = HeldButtons::default();
        held.set(BUTTONS, LEFT, LEFT);
        held.set(BUTTONS, LEFT, Keys::empty());
        let change = held.set(BUTTONS, LEFT, LEFT);
        assert!(change.pressed.is_empty(), "it never went up on the game");
        assert!(held.take_owed().is_empty());
        assert!(held.held().contains(LEFT));
    }

    #[test]
    fn an_unplugged_pad_lets_go_of_everything_it_held() {
        let mut held = HeldButtons::default();
        held.set(BUTTONS, Keys::A, Keys::A);
        held.set(STICK, LEFT | RIGHT, LEFT);
        held.set(InputSource::PadButtons(1), Keys::B, Keys::B);
        played(&mut held);
        let change = held.forget_pad(0);
        assert_eq!(change.released.bits(), (Keys::A | LEFT).bits());
        assert_eq!(held.held().bits(), Keys::B.bits());
    }

    #[test]
    fn clearing_lets_go_at_once() {
        let mut held = HeldButtons::default();
        held.set(BUTTONS, LEFT, LEFT);
        held.set(BUTTONS, LEFT, Keys::empty());
        held.set(InputSource::Touch, RIGHT, RIGHT);
        let change = held.clear();
        assert_eq!(change.released.bits(), (LEFT | RIGHT).bits());
        assert!(held.held().is_empty());
        assert!(held.take_owed().is_empty());
    }
}

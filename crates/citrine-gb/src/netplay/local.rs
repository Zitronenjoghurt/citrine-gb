use super::input::Input;
use super::pair::{self, Cable};
use crate::gb::GameBoy;
use crate::gb::joypad::JoypadState;

#[derive(Debug, Default)]
pub struct LocalCable {
    cable: Cable,
}

impl LocalCable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn plugged(&self) -> bool {
        self.cable.plugged
    }

    pub fn set_plugged(&mut self, plugged: bool) {
        self.cable.plugged = plugged;
    }

    pub fn run_frame(&mut self, consoles: [&mut GameBoy; 2], buttons: [JoypadState; 2]) {
        pair::run_pair(
            consoles,
            &mut self.cable,
            &buttons.map(Input::new),
            [true; 2],
        );
    }

    pub fn unplug(&mut self, mut consoles: [&mut GameBoy; 2]) {
        self.cable.plugged = false;
        if consoles.iter().any(|gb| gb.link_plugged()) {
            pair::disconnect(&mut consoles, &mut self.cable);
        }
    }
}

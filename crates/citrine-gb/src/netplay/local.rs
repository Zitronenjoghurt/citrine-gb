use super::input::Input;
use super::pair::{self, Cable, LinkPort};
use crate::console::{Console, Keys};

#[derive(Debug)]
pub struct LocalCable<C: LinkPort> {
    cable: Cable<C::Message>,
}

impl<C: LinkPort> Default for LocalCable<C> {
    fn default() -> Self {
        Self {
            cable: Cable::default(),
        }
    }
}

impl<C: Console + LinkPort> LocalCable<C> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn plugged(&self) -> bool {
        self.cable.plugged
    }

    pub fn set_plugged(&mut self, plugged: bool) {
        self.cable.plugged = plugged;
    }

    pub fn run_frame(&mut self, consoles: [&mut C; 2], keys: [Keys; 2]) {
        pair::run_pair(consoles, &mut self.cable, &keys.map(Input::new), [true; 2]);
    }

    pub fn unplug(&mut self, mut consoles: [&mut C; 2]) {
        self.cable.plugged = false;
        if consoles.iter().any(|console| console.link_plugged()) {
            pair::disconnect(&mut consoles, &mut self.cable);
        }
    }
}

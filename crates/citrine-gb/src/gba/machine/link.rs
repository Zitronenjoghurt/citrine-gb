use super::{CYCLES_PER_FRAME, Gba};
use crate::gba::serial::LinkMessage;
use crate::netplay::LinkPort;

impl LinkPort for Gba {
    type Message = LinkMessage;

    fn plug_link(&mut self) {
        self.plug_link_as(0, 2);
    }

    fn plug_link_as(&mut self, unit: usize, units: usize) {
        self.bus.plug_link(unit, units);
    }

    fn unplug_link(&mut self) {
        self.bus.unplug_link();
    }

    fn link_plugged(&self) -> bool {
        self.bus.serial().plugged()
    }

    fn link_waiting(&self) -> bool {
        self.bus.serial().waiting()
    }

    fn take_link_messages(&mut self) -> Vec<LinkMessage> {
        self.bus.take_link_messages()
    }

    fn receive_link_message(&mut self, message: LinkMessage) {
        self.bus.receive_link_message(message);
    }

    fn is_transfer(message: &LinkMessage) -> bool {
        message.is_transfer()
    }

    fn begin_linked_frame(&mut self) -> u64 {
        let fresh = self.frame_end == 0;
        self.catch_up_frame_end();
        if fresh {
            self.cycles - (self.frame_end - CYCLES_PER_FRAME)
        } else {
            0
        }
    }

    fn step_linked(&mut self) -> u64 {
        u64::from(self.step())
    }

    fn linked_frame_done(&self) -> bool {
        self.fault.is_some() || self.cycles >= self.frame_end
    }

    fn end_linked_frame(&mut self) {
        self.rumble = self.bus.cart_mut().take_rumble();
    }
}

#[cfg(test)]
mod tests;

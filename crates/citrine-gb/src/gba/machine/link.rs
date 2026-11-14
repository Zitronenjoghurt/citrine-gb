use super::{CYCLES_PER_FRAME, Gba};
use crate::gba::serial::LinkMessage;
use crate::netplay::LinkPort;

impl LinkPort for Gba {
    type Message = LinkMessage;
    const CABLE_WINDOWS: bool = true;

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

    fn aged(message: LinkMessage, late: u64) -> LinkMessage {
        message.aged(late)
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
mod tests {
    use super::*;
    use crate::console::Keys;
    use crate::netplay::pair::{Cable, run_pair};
    use crate::netplay::{CableEvent, Input};

    const EWRAM: u32 = 0x0200_0000;
    const MASTER: u32 = 0x1081;
    const SLAVE: u32 = 0x1080;
    const WORDS: u32 = 48;

    fn transfer_program(seed: u32, control: u32, delay: u32) -> Vec<u8> {
        [
            0xE3A0_0404,
            0xE280_3C01,
            0xE3A0_9000,
            0xE1C3_93B4,
            0xE3A0_5402,
            0xE59F_1040,
            0xE59F_2040,
            0xE59F_4040,
            0xE3A0_6000 | WORDS,
            0xE583_1020,
            0xE1A0_7004,
            0xE257_7001,
            0x1AFF_FFFD,
            0xE1C3_22B8,
            0xE1D3_82B8,
            0xE318_0080,
            0x1AFF_FFFC,
            0xE593_8020,
            0xE485_8004,
            0xE281_1001,
            0xE256_6001,
            0x1AFF_FFF2,
            0xEAFF_FFFE,
            seed,
            control,
            delay,
        ]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect()
    }

    fn received(gba: &Gba) -> Vec<u32> {
        (0..WORDS)
            .map(|i| gba.bus().read32(EWRAM + 4 * i))
            .collect()
    }

    fn sent(seed: u32) -> Vec<u32> {
        (0..WORDS).map(|i| seed + i).collect()
    }

    #[test]
    fn two_gbas_swap_a_run_of_words_over_the_pair() {
        let mut one = Gba::new(&transfer_program(0x1234_5600, MASTER, 0x100));
        let mut two = Gba::new(&transfer_program(0x9ABC_DE00, SLAVE, 1));
        let mut cable = Cable::default();
        let plug = Input::new(Keys::empty()).with_cable(CableEvent::Plug);
        let idle = Input::new(Keys::empty());
        run_pair([&mut one, &mut two], &mut cable, &[plug, idle], [true; 2]);
        for _ in 0..4 {
            run_pair([&mut one, &mut two], &mut cable, &[idle, idle], [true; 2]);
        }
        assert!(one.link_plugged() && two.link_plugged());
        assert_eq!(one.fault(), None);
        assert_eq!(two.fault(), None);
        assert_eq!(received(&one), sent(0x9ABC_DE00));
        assert_eq!(received(&two), sent(0x1234_5600));
        assert_eq!(cable.transfers, [u64::from(WORDS), 0]);
    }

    #[test]
    fn a_linked_frame_runs_as_long_as_a_plain_one() {
        let mut linked = Gba::new(&transfer_program(0, SLAVE, 1));
        let mut plain = Gba::new(&transfer_program(0, SLAVE, 1));
        linked.plug_link_as(1, 2);
        linked.begin_linked_frame();
        while !linked.linked_frame_done() {
            linked.step_linked();
        }
        linked.end_linked_frame();
        plain.run_frame();
        assert_eq!(linked.cycles(), plain.cycles());
        plain.run_frame();
        linked.unplug_link();
        linked.run_frame();
        assert_eq!(linked.cycles(), plain.cycles());
    }
}

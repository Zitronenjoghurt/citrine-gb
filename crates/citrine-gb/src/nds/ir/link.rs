use std::collections::VecDeque;

use super::IrPort;
use crate::nds::rtc::CYCLES_PER_SECOND;

const BAUD: u64 = 115_200;
const BITS_PER_BYTE: u64 = 10;
const SIDES: usize = 2;

pub fn airtime(bytes: usize) -> u64 {
    (bytes as u64 * BITS_PER_BYTE * u64::from(CYCLES_PER_SECOND)).div_ceil(BAUD)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Window {
    start: u64,
    end: u64,
}

impl Window {
    fn overlaps(self, other: Window) -> bool {
        self.start < other.end && other.start < self.end
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Flight {
    window: Window,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Side {
    inbox: VecDeque<Flight>,
    sending: Vec<Window>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct IrLink {
    sides: [Side; SIDES],
}

impl IrLink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn end(&mut self, side: usize) -> impl IrPort + '_ {
        End { link: self, side }
    }
}

struct End<'a> {
    link: &'a mut IrLink,
    side: usize,
}

impl IrPort for End<'_> {
    fn send(&mut self, packet: &[u8], at: u64) {
        let own = &mut self.link.sides[self.side];
        let start = own.sending.last().map_or(at, |last| at.max(last.end));
        let window = Window {
            start,
            end: start + airtime(packet.len()),
        };
        own.sending.push(window);
        self.link.sides[1 - self.side].inbox.push_back(Flight {
            window,
            bytes: packet.to_vec(),
        });
    }

    fn take(&mut self, now: u64) -> Option<Vec<u8>> {
        let own = &mut self.link.sides[self.side];
        let mut taken = None;
        while taken.is_none() && own.inbox.front().is_some_and(|f| f.window.end <= now) {
            let flight = own.inbox.pop_front()?;
            if !own.sending.iter().any(|&w| w.overlaps(flight.window)) {
                taken = Some(flight.bytes);
            }
        }
        let oldest = own.inbox.front().map_or(now, |f| f.window.start.min(now));
        own.sending.retain(|w| w.end > oldest);
        taken
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nds::ir::{IrChip, IrVersion};
    use crate::nds::save::{Backup, SaveType};

    const RECEIVE: u8 = 0x01;
    const SEND: u8 = 0x02;
    const BYTE: u64 = 2_910;

    fn chip(version: IrVersion) -> IrChip {
        IrChip::new(version, Backup::new(SaveType::Flash512K))
    }

    fn command(chip: &mut IrChip, port: &mut dyn IrPort, now: u64, bytes: &[u8]) -> Vec<u8> {
        let out = bytes
            .iter()
            .map(|&byte| chip.transfer(byte, port, now))
            .collect();
        chip.deselect(port, now);
        out
    }

    fn send(chip: &mut IrChip, port: &mut dyn IrPort, now: u64, packet: &[u8]) {
        command(chip, port, now, &[&[SEND], packet].concat());
    }

    fn receive(chip: &mut IrChip, port: &mut dyn IrPort, now: u64) -> Vec<u8> {
        let out = command(chip, port, now, &[RECEIVE; 0x100]);
        let len = usize::from(out[1]);
        out[2..2 + len].to_vec()
    }

    #[test]
    fn a_byte_takes_ten_bits_at_115200_baud() {
        assert_eq!(airtime(1), BYTE);
        assert_eq!(airtime(136), 395_652);
    }

    #[test]
    fn packets_cross_both_ways() {
        let mut link = IrLink::new();
        let (mut game, mut other) = (chip(IrVersion::New), chip(IrVersion::New));
        send(&mut game, &mut link.end(0), 1_000, &[0xFC]);
        assert_eq!(receive(&mut other, &mut link.end(1), 1_000 + BYTE), [0xFC]);
        let reply: Vec<u8> = (0..136).map(|i| i as u8 ^ 0xAA).collect();
        send(&mut other, &mut link.end(1), 50_000, &reply);
        assert_eq!(
            receive(&mut game, &mut link.end(0), 50_000 + airtime(136)),
            reply
        );
    }

    #[test]
    fn nothing_pending_answers_length_zero() {
        let mut link = IrLink::new();
        let mut game = chip(IrVersion::New);
        assert_eq!(
            command(&mut game, &mut link.end(0), 0, &[RECEIVE, 0, 0]),
            [0, 0, 0]
        );
    }

    #[test]
    fn a_packet_is_held_until_its_last_byte_arrives() {
        let mut link = IrLink::new();
        let (mut game, mut other) = (chip(IrVersion::New), chip(IrVersion::New));
        send(&mut game, &mut link.end(0), 0, &[1, 2, 3]);
        let whole = airtime(3);
        assert!(receive(&mut other, &mut link.end(1), whole - 1).is_empty());
        assert_eq!(receive(&mut other, &mut link.end(1), whole), [1, 2, 3]);
        assert!(receive(&mut other, &mut link.end(1), whole).is_empty());
    }

    #[test]
    fn sending_starts_when_chip_select_is_released() {
        let mut link = IrLink::new();
        let (mut game, mut other) = (chip(IrVersion::New), chip(IrVersion::New));
        for byte in [SEND, 7, 8] {
            game.transfer(byte, &mut link.end(0), 0);
        }
        game.deselect(&mut link.end(0), 10_000);
        let arrives = 10_000 + airtime(2);
        assert!(receive(&mut other, &mut link.end(1), arrives - 1).is_empty());
        assert_eq!(receive(&mut other, &mut link.end(1), arrives), [7, 8]);
    }

    #[test]
    fn back_to_back_sends_queue_on_the_line() {
        let mut link = IrLink::new();
        let (mut game, mut other) = (chip(IrVersion::New), chip(IrVersion::New));
        send(&mut game, &mut link.end(0), 0, &[1, 2]);
        send(&mut game, &mut link.end(0), 0, &[3]);
        assert_eq!(receive(&mut other, &mut link.end(1), airtime(2)), [1, 2]);
        let second = airtime(2) + BYTE;
        assert!(receive(&mut other, &mut link.end(1), second - 1).is_empty());
        assert_eq!(receive(&mut other, &mut link.end(1), second), [3]);
    }

    #[test]
    fn packets_wait_in_order_until_taken() {
        let mut link = IrLink::new();
        let (mut game, mut other) = (chip(IrVersion::New), chip(IrVersion::New));
        send(&mut game, &mut link.end(0), 0, &[1]);
        send(&mut game, &mut link.end(0), 100_000, &[2]);
        let late = 1_000_000;
        assert_eq!(receive(&mut other, &mut link.end(1), late), [1]);
        assert_eq!(receive(&mut other, &mut link.end(1), late), [2]);
    }

    #[test]
    fn packets_that_cross_on_the_line_are_lost_on_both_sides() {
        let mut link = IrLink::new();
        let (mut game, mut other) = (chip(IrVersion::New), chip(IrVersion::New));
        send(&mut game, &mut link.end(0), 0, &[1, 2, 3, 4]);
        send(&mut other, &mut link.end(1), BYTE, &[9]);
        send(&mut other, &mut link.end(1), airtime(4), &[10]);
        let late = 1_000_000;
        assert_eq!(receive(&mut game, &mut link.end(0), late), [10]);
        assert!(receive(&mut other, &mut link.end(1), late).is_empty());
    }

    #[test]
    fn packets_stop_at_the_chip_limit() {
        for (version, limit) in [(IrVersion::Old, 0x84), (IrVersion::New, 0xB8)] {
            let mut link = IrLink::new();
            let (mut game, mut other) = (chip(version), chip(version));
            let long: Vec<u8> = (0..0xC0).map(|i| i as u8).collect();
            send(&mut game, &mut link.end(0), 0, &long);
            assert_eq!(
                receive(&mut other, &mut link.end(1), 1 << 30),
                long[..limit]
            );
        }
    }

    #[test]
    fn a_longer_packet_from_outside_is_cut_to_the_limit() {
        let mut link = IrLink::new();
        let mut game = chip(IrVersion::Old);
        let long = vec![0x55; 0xA0];
        link.end(1).send(&long, 0);
        assert_eq!(receive(&mut game, &mut link.end(0), 1 << 30).len(), 0x84);
    }

    #[test]
    fn the_old_chip_refuses_to_send_f2_first() {
        let mut link = IrLink::new();
        let (mut game, mut other) = (chip(IrVersion::Old), chip(IrVersion::Old));
        send(&mut game, &mut link.end(0), 0, &[0xF2, 1]);
        send(&mut game, &mut link.end(0), 0, &[1, 0xF2]);
        assert_eq!(receive(&mut other, &mut link.end(1), 1 << 30), [1, 0xF2]);
        assert!(receive(&mut other, &mut link.end(1), 1 << 30).is_empty());
    }

    #[test]
    fn the_new_chip_sends_f2_first() {
        let mut link = IrLink::new();
        let (mut game, mut other) = (chip(IrVersion::New), chip(IrVersion::New));
        send(&mut game, &mut link.end(0), 0, &[0xF2]);
        assert_eq!(receive(&mut other, &mut link.end(1), 1 << 30), [0xF2]);
    }

    #[test]
    fn an_empty_send_puts_nothing_on_the_line() {
        let mut link = IrLink::new();
        let (mut game, mut other) = (chip(IrVersion::New), chip(IrVersion::New));
        send(&mut game, &mut link.end(0), 0, &[]);
        send(&mut game, &mut link.end(0), 0, &[5]);
        assert_eq!(receive(&mut other, &mut link.end(1), BYTE), [5]);
    }

    #[test]
    fn save_access_through_the_link_sends_nothing() {
        let mut link = IrLink::new();
        let (mut game, mut other) = (chip(IrVersion::New), chip(IrVersion::New));
        command(&mut game, &mut link.end(0), 0, &[0x00, 0x06]);
        command(&mut game, &mut link.end(0), 0, &[0x00, 0x02, 0, 0, 0, 0x42]);
        assert!(receive(&mut other, &mut link.end(1), 1 << 30).is_empty());
        assert!(game.save().dirty());
    }
}

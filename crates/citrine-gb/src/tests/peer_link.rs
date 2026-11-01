use super::local_link::{COUNT, done, red, saved, sent};
use crate::gb::joypad::JoypadState;
use crate::netplay::{LinkPeer, PeerCable};

#[derive(Default)]
struct Echo {
    heard: Vec<u8>,
}

impl LinkPeer for Echo {
    fn exchange(&mut self, sent: u8) -> u8 {
        self.heard.push(sent);
        !sent
    }
}

#[test]
fn a_peer_answers_every_byte_the_game_clocks() {
    let mut gb = red();
    gb.press_button(JoypadState::A);
    let mut cable = PeerCable::new();
    cable.set_plugged(true);
    let mut peer = Echo::default();
    let frames = (1..=600)
        .find(|_| {
            cable.run_frame(&mut gb, &mut peer);
            done(&gb)
        })
        .expect("the game never finished its transfers");
    assert!(frames < 120);
    assert_eq!(peer.heard, sent(0x10, 1));
    let answered: Vec<u8> = sent(0x10, 1).iter().map(|b| !b).collect();
    assert_eq!(saved(&mut gb), answered);
    assert_eq!(cable.transfers(), u64::from(COUNT));

    cable.unplug(&mut gb);
    assert!(!cable.plugged() && !gb.link_plugged());
}

#[test]
fn an_unplugged_peer_cable_leaves_the_game_alone() {
    let mut gb = red();
    gb.press_button(JoypadState::A);
    let mut cable = PeerCable::new();
    let mut peer = Echo::default();
    for _ in 0..120 {
        cable.run_frame(&mut gb, &mut peer);
    }
    assert!(done(&gb));
    assert!(peer.heard.is_empty());
    assert_eq!(saved(&mut gb), vec![0xFF; COUNT as usize]);
}

use crate::gb::GameBoy;
use crate::gb::serial::LinkMessage;

pub trait LinkPeer {
    fn exchange(&mut self, sent: u8) -> u8;
}

#[derive(Debug, Default)]
pub struct PeerCable {
    plugged: bool,
    transfers: u64,
}

impl PeerCable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn plugged(&self) -> bool {
        self.plugged
    }

    pub fn set_plugged(&mut self, plugged: bool) {
        self.plugged = plugged;
    }

    pub fn transfers(&self) -> u64 {
        self.transfers
    }

    pub fn run_frame(&mut self, gb: &mut GameBoy, peer: &mut dyn LinkPeer) {
        if self.plugged != gb.link_plugged() {
            if self.plugged {
                gb.plug_link();
            } else {
                gb.unplug_link();
            }
        }
        if !self.plugged {
            gb.run_frame();
            return;
        }

        gb.ppu.frame_ready = false;
        while !gb.ppu.frame_ready {
            gb.step();
            for message in gb.take_link_messages() {
                if let LinkMessage::Transfer { id, byte, .. } = message {
                    self.transfers += 1;
                    let reply = peer.exchange(byte);
                    gb.receive_link_message(LinkMessage::Reply { id, byte: reply });
                }
            }

            #[cfg(feature = "debug")]
            if gb.at_breakpoint() {
                gb.debugger.hit_breakpoint = true;
                return;
            }

            if !gb.ppu.lcdc.lcd_enabled && gb.cycle_counter >= gb.frame_cycles() {
                break;
            }
        }
        if gb.cycle_counter >= gb.frame_cycles() {
            gb.cycle_counter -= gb.frame_cycles();
        }
        gb.apu.flush_audio();
    }

    pub fn unplug(&mut self, gb: &mut GameBoy) {
        self.plugged = false;
        if gb.link_plugged() {
            gb.unplug_link();
        }
    }
}

#[cfg(all(test, feature = "persistence"))]
mod tests {
    use crate::gb::joypad::JoypadState;
    use crate::netplay::{LinkPeer, PeerCable};
    use crate::test_support::{COUNT, done, red, saved, sent};

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
}

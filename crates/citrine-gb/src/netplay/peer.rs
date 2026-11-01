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

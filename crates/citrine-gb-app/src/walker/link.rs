use citrine_gb::nds::ir::IrPort;
use citrine_gb_walker::{Event, Link, Walker};
use std::fmt::Write;

pub struct WalkerLink {
    walker: Walker,
    connect: bool,
}

impl WalkerLink {
    pub(crate) fn new(walker: Walker) -> Self {
        Self {
            walker,
            connect: false,
        }
    }

    pub fn walker(&self) -> &Walker {
        &self.walker
    }

    pub fn walker_mut(&mut self) -> &mut Walker {
        &mut self.walker
    }

    pub fn request_connect(&mut self) {
        self.connect = true;
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        self.walker.events()
    }

    pub(crate) fn into_walker(self) -> Walker {
        Walker::restore(self.walker.eeprom().clone(), self.walker.memory())
    }

    fn follow_connect(&mut self, now: u64) {
        if std::mem::take(&mut self.connect) && self.walker.link() == Link::Idle {
            log::debug!("walker ir {now}: advertising");
            self.walker.connect(now);
        }
    }
}

impl IrPort for WalkerLink {
    fn send(&mut self, packet: &[u8], at: u64) {
        self.follow_connect(at);
        log::debug!("walker ir {at}: game sent {}", hex(packet));
        self.walker.send(packet, at);
    }

    fn take(&mut self, now: u64) -> Option<Vec<u8>> {
        self.follow_connect(now);
        let packet = self.walker.take(now)?;
        log::debug!("walker ir {now}: walker sent {}", hex(&packet));
        Some(packet)
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 3);
    for (i, byte) in bytes.iter().enumerate() {
        if i > 0 {
            text.push(' ');
        }
        let _ = write!(text, "{byte:02X}");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb_walker::CYCLES_PER_SECOND;

    #[test]
    fn a_connect_request_starts_advertising_at_the_next_ir_access() {
        let mut link = WalkerLink::new(Walker::new(1));
        link.request_connect();
        assert_eq!(link.walker().link(), Link::Idle);
        let now = CYCLES_PER_SECOND;
        assert_eq!(link.take(now), Some(vec![0x56]));
        assert_eq!(link.walker().link(), Link::Advertising);
    }

    #[test]
    fn handing_back_mid_session_drops_the_session_and_keeps_the_eeprom() {
        let mut link = WalkerLink::new(Walker::new(9));
        link.walker_mut().steps(300);
        link.request_connect();
        link.take(CYCLES_PER_SECOND);
        let before = link.walker().eeprom().clone();
        let memory = link.walker().memory();
        let walker = link.into_walker();
        assert_eq!(walker.link(), Link::Idle);
        assert_eq!(walker.eeprom(), &before);
        assert_eq!(walker.memory(), memory);
    }

    #[test]
    fn packets_log_as_spaced_hex() {
        assert_eq!(hex(&[0x56, 0x0A, 0xFF]), "56 0A FF");
        assert_eq!(hex(&[]), "");
    }
}

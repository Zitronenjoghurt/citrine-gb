use crate::gb::bus::CpuBusInterface;
use crate::gb::ic::{ICInterface, Interrupt};
use std::collections::HashMap;
use std::io::Write;

mod audio;
mod boot_rom;
mod cgb;
#[cfg(feature = "debug")]
mod code;
mod cpu;
#[cfg(feature = "debug")]
mod debug_step;
#[cfg(feature = "debug")]
mod disassembly;
mod e2e;
mod halt;
mod keys;
mod lcd_on;
mod link;
#[cfg(feature = "persistence")]
mod local_link;
#[cfg(all(feature = "debug", feature = "persistence"))]
mod machine;
#[cfg(feature = "persistence")]
mod mappers;
#[cfg(feature = "debug")]
mod memory_view;
#[cfg(feature = "persistence")]
mod netplay;
#[cfg(feature = "persistence")]
mod peer_link;
#[cfg(feature = "debug")]
mod registers;
#[cfg(feature = "persistence")]
mod rooms;
#[cfg(feature = "persistence")]
mod rtc;
#[cfg(feature = "persistence")]
mod snapshot;
#[cfg(feature = "debug")]
mod trace;
#[cfg(feature = "debug")]
mod video;
#[cfg(all(feature = "debug", feature = "gba"))]
mod video_gba;

#[derive(Debug, Default, Eq, PartialEq)]
pub struct TestBus {
    data: HashMap<u16, u8>,
    history: Vec<(u16, u8, String)>,
}

impl CpuBusInterface for TestBus {
    fn cycle(&mut self) {}

    fn read(&mut self, addr: u16) -> u8 {
        let value = self
            .data
            .get(&addr)
            .copied()
            .unwrap_or_else(|| panic!("No data for address {:04X}", addr));
        self.history.push((addr, value, "read".to_string()));
        value
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.data.insert(addr, value);
        self.history.push((addr, value, "write".to_string()));
    }
}

impl ICInterface for TestBus {
    fn request_interrupt(&mut self, _interrupt: Interrupt) {}
}

#[cfg(feature = "debug")]
impl crate::debug::DebuggerInterface for TestBus {}

pub fn skip(test: &str, reason: &str) {
    let _ = writeln!(std::io::stderr(), "\nskipped {test}: {reason}");
}

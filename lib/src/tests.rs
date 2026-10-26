use crate::gb::bus::CpuBusInterface;
use crate::gb::ic::{ICInterface, Interrupt};
use std::collections::HashMap;
use std::io::Write;

mod audio;
mod boot_rom;
mod cgb;
mod cpu;
mod e2e;
mod halt;
mod lcd_on;
#[cfg(feature = "persistence")]
mod rtc;
#[cfg(feature = "persistence")]
mod snapshot;

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

use crate::gb::bus::BusInterface;
use crate::gb::ic::{ICInterface, Interrupt};
use std::collections::HashMap;

mod cpu;

#[derive(Debug, Default, Eq, PartialEq)]
pub struct TestBus {
    data: HashMap<u16, u8>,
    history: Vec<(u16, u8, String)>,
}

impl BusInterface for TestBus {
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

use crate::gb::bus::BusInterface;
use std::collections::HashMap;

mod cpu;

#[derive(Debug, Default, Eq, PartialEq)]
pub struct TestBus {
    data: HashMap<u16, u8>,
}

impl TestBus {
    pub fn new(data: &[(u16, u8)]) -> Self {
        Self {
            data: data.iter().copied().collect(),
        }
    }
}

impl BusInterface for TestBus {
    fn cycle(&mut self) {}

    fn read(&mut self, addr: u16) -> u8 {
        self.data
            .get(&addr)
            .copied()
            .unwrap_or_else(|| panic!("No data for address {:04X}", addr))
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.data.insert(addr, value);
    }
}

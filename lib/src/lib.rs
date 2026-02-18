mod disassembler;
pub mod gb;
mod instructions;
pub mod rom;
#[cfg(test)]
mod tests;
mod utils;

pub trait ReadMemory {
    fn read_naive(&self, addr: u16) -> u8;
}

impl ReadMemory for &[u8] {
    fn read_naive(&self, addr: u16) -> u8 {
        self.get(addr as usize).copied().unwrap_or(0xFF)
    }
}

pub trait WriteMemory {
    fn write_naive(&mut self, addr: u16, value: u8);
}

impl WriteMemory for &mut [u8] {
    fn write_naive(&mut self, addr: u16, value: u8) {
        if let Some(byte) = self.get_mut(addr as usize) {
            *byte = value;
        }
    }
}

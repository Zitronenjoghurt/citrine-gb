use super::Nds;
use crate::cheats::CheatTarget;
use crate::gba::wait::Width;

fn width(bytes: u8) -> Width {
    match bytes {
        1 => Width::Byte,
        2 => Width::Half,
        _ => Width::Word,
    }
}

impl CheatTarget for Nds {
    fn cheat_read(&mut self, address: u32, bytes: u8) -> u32 {
        self.buses.arm7().read(address, width(bytes))
    }

    fn cheat_write(&mut self, address: u32, bytes: u8, value: u32) {
        self.buses.arm7().write(address, width(bytes), value);
    }

    fn cheat_rom(&self) -> &[u8] {
        &[]
    }

    fn rom_offsets(&self, _address: u32, _out: &mut Vec<usize>) {}

    fn set_rom_patches(&mut self, _patches: &[(usize, u8)]) {}
}

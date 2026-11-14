use super::Gba;
use crate::cheats::CheatTarget;
use crate::gba::region::Region;
use crate::gba::wait::Width;

fn width(bytes: u8) -> Width {
    match bytes {
        1 => Width::Byte,
        2 => Width::Half,
        _ => Width::Word,
    }
}

impl CheatTarget for Gba {
    fn cheat_read(&mut self, address: u32, bytes: u8) -> u32 {
        self.bus.read(address, width(bytes))
    }

    fn cheat_write(&mut self, address: u32, bytes: u8, value: u32) {
        self.bus.write(address, width(bytes), value);
    }

    fn cheat_rom(&self) -> &[u8] {
        self.bus.pristine_rom()
    }

    fn rom_offsets(&self, address: u32, out: &mut Vec<usize>) {
        if let region @ Region::Rom(_) = Region::of(address) {
            out.push(region.offset(address));
        }
    }

    fn set_rom_patches(&mut self, patches: &[(usize, u8)]) {
        if self.bus.set_rom_patches(patches) {
            self.idle.forget();
        }
    }
}

use super::GameBoy;
use super::cartridge::ROM_BANK_SIZE;
use crate::cheats::CheatTarget;
use crate::cheats::gb::BANK_SHIFT;
use crate::{ReadMemory, WriteMemory};

impl GameBoy {
    fn cheat_peek(&self, address: u32) -> u8 {
        let addr = address as u16;
        let bank = (address >> BANK_SHIFT) as usize;
        match addr {
            0xD000..=0xDFFF if bank > 0 => self
                .memory
                .wram_bank(bank - 1)
                .map_or(0xFF, |bytes| bytes[usize::from(addr - 0xD000)]),
            0x0000..=0x7FFF | 0xA000..=0xBFFF => self.cartridge.read_naive(addr),
            0x8000..=0x9FFF | 0xFE00..=0xFE9F => self.ppu.read_naive(addr),
            0xC000..=0xFDFF | 0xFF80..=0xFFFE => self.memory.read_naive(addr),
            _ => 0xFF,
        }
    }

    fn cheat_poke(&mut self, address: u32, value: u8) {
        let addr = address as u16;
        let bank = (address >> BANK_SHIFT) as usize;
        match addr {
            0xD000..=0xDFFF if bank > 0 => {
                if let Some(bytes) = self.memory.wram_bank_mut(bank - 1) {
                    bytes[usize::from(addr - 0xD000)] = value;
                }
            }
            0xA000..=0xBFFF if self.cartridge.read_naive(addr) != value => {
                self.cartridge.write_naive(addr, value)
            }
            0x8000..=0x9FFF | 0xFE00..=0xFE9F => self.ppu.write_naive(addr, value),
            0xC000..=0xFDFF | 0xFF80..=0xFFFE => self.memory.write_naive(addr, value),
            _ => {}
        }
    }
}

impl CheatTarget for GameBoy {
    fn cheat_read(&mut self, address: u32, width: u8) -> u32 {
        (0..u32::from(width)).fold(0, |value, index| {
            value | u32::from(self.cheat_peek(address.wrapping_add(index))) << (8 * index)
        })
    }

    fn cheat_write(&mut self, address: u32, width: u8, value: u32) {
        for index in 0..u32::from(width) {
            self.cheat_poke(address.wrapping_add(index), (value >> (8 * index)) as u8);
        }
    }

    fn cheat_rom(&self) -> &[u8] {
        self.cartridge.pristine_rom()
    }

    fn rom_offsets(&self, address: u32, out: &mut Vec<usize>) {
        let address = address as usize;
        match address {
            0..ROM_BANK_SIZE => out.push(address),
            ROM_BANK_SIZE..0x8000 => {
                let banks = self.cartridge.pristine_rom().len() / ROM_BANK_SIZE;
                out.extend((0..banks).map(|bank| bank * ROM_BANK_SIZE + address - ROM_BANK_SIZE));
            }
            _ => {}
        }
    }

    fn set_rom_patches(&mut self, patches: &[(usize, u8)]) {
        self.cartridge.set_rom_patches(patches);
    }
}

#[cfg(test)]
mod tests {
    use crate::ReadMemory;
    use crate::cheats::{Cheat, apply};
    use crate::console::{Console, ConsoleKind};
    use crate::gb::{GameBoy, GbModel};
    use crate::rom::Rom;

    fn console(model: GbModel, byte_at_4a17: u8) -> GameBoy {
        let mut data = vec![0u8; 0x10000];
        data[0x0100..0x0104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]);
        data[0x0143] = if model == GbModel::Cgb { 0x80 } else { 0 };
        data[0x0147] = 0x19;
        data[0x0148] = 0x01;
        data[0x0150..0x0152].copy_from_slice(&[0x18, 0xFE]);
        data[0x4A17] = byte_at_4a17;
        data[0xCA17] = byte_at_4a17;
        let mut gb = GameBoy::new_empty(model);
        gb.load_rom(&Rom::new(&data)).expect("load");
        gb
    }

    fn read(gb: &mut GameBoy, addr: u16) -> u8 {
        gb.with_bus(|bus| bus.read_naive(addr))
    }

    fn cheat(code: &str) -> Cheat {
        Cheat::parse(ConsoleKind::Gbc, code).expect("code")
    }

    #[test]
    fn a_game_genie_code_patches_every_bank_whose_byte_matches_and_keeps_the_rom_file_clean() {
        let mut gb = console(GbModel::Dmg, 0xC8);
        apply(&mut gb, &[cheat("00A-17B-C49")]);
        assert_eq!(read(&mut gb, 0x4A17), 0x00);
        assert_eq!(gb.cartridge.pristine_rom()[0xCA17], 0xC8);
        assert_eq!(gb.rom_bytes()[0x4A17], 0xC8);
        apply(&mut gb, &[]);
        assert_eq!(read(&mut gb, 0x4A17), 0xC8);
    }

    #[test]
    fn a_game_genie_code_leaves_a_byte_that_does_not_match_its_compare_alone() {
        let mut gb = console(GbModel::Dmg, 0x11);
        apply(&mut gb, &[cheat("00A-17B-C49")]);
        assert_eq!(read(&mut gb, 0x4A17), 0x11);
        apply(&mut gb, &[cheat("00A-17B")]);
        assert_eq!(read(&mut gb, 0x4A17), 0x00);
    }

    #[test]
    fn a_game_shark_code_writes_work_ram_again_every_frame() {
        let mut gb = console(GbModel::Dmg, 0);
        let codes = [cheat("01FF47D3")];
        apply(&mut gb, &codes);
        assert_eq!(read(&mut gb, 0xD347), 0xFF);
        gb.with_bus(|bus| crate::WriteMemory::write_naive(bus, 0xD347, 0x12));
        gb.run_frame();
        apply(&mut gb, &codes);
        assert_eq!(read(&mut gb, 0xD347), 0xFF);
    }

    #[test]
    fn a_game_shark_code_for_a_color_work_ram_bank_writes_that_bank_only() {
        let mut gb = console(GbModel::Cgb, 0);
        apply(&mut gb, &[cheat("9363C9D8")]);
        assert_eq!(read(&mut gb, 0xD8C9), 0x00);
        assert_eq!(gb.memory.wram_bank(3).expect("bank")[0x8C9], 0x63);
    }
}

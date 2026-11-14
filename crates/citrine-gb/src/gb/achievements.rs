use super::GameBoy;
use super::cartridge::RAM_BANK_SIZE;
use crate::ReadMemory;

const WRAM_BANK_SIZE: u32 = 0x1000;

impl GameBoy {
    pub fn achievement_peek(&self, address: u32) -> Option<u8> {
        match address {
            0x0000..=0x7FFF => Some(self.cartridge.read_naive(address as u16)),
            0x8000..=0x9FFF | 0xFE00..=0xFE9F => Some(self.ppu.read_naive(address as u16)),
            0xA000..=0xBFFF => Some(self.cart_ram_byte(0, address - 0xA000)),
            0xC000..=0xDFFF => {
                let bank = ((address - 0xC000) / WRAM_BANK_SIZE) as usize;
                self.wram_byte(bank, address % WRAM_BANK_SIZE)
            }
            0xE000..=0xFDFF => self.achievement_peek(address - 0x2000),
            0xFEA0..=0xFEFF => Some(0),
            0xFF00..=0xFFFF => Some(self.memory.read_naive(address as u16)),
            0x1_0000..=0x1_5FFF => {
                let bank = 2 + ((address - 0x1_0000) / WRAM_BANK_SIZE) as usize;
                Some(self.wram_byte(bank, address % WRAM_BANK_SIZE).unwrap_or(0))
            }
            0x1_6000..=0x3_3FFF => {
                let offset = address - 0x1_6000;
                let bank = 1 + (offset / RAM_BANK_SIZE as u32) as usize;
                Some(self.cart_ram_byte(bank, offset % RAM_BANK_SIZE as u32))
            }
            _ => None,
        }
    }

    pub fn achievement_read(&self, address: u32, buffer: &mut [u8]) -> usize {
        let mut done = 0;
        while done < buffer.len() {
            let at = address + done as u32;
            let left = &mut buffer[done..];
            let copied = match self.achievement_slice(at) {
                Some(bytes) => {
                    let n = bytes.len().min(left.len());
                    left[..n].copy_from_slice(&bytes[..n]);
                    n
                }
                None => match self.achievement_peek(at) {
                    Some(value) => {
                        left[0] = value;
                        1
                    }
                    None => return done,
                },
            };
            done += copied;
        }
        done
    }

    fn achievement_slice(&self, address: u32) -> Option<&[u8]> {
        let (bytes, offset): (&[u8], u32) = match address {
            0xA000..=0xBFFF => (self.cartridge.ram_banks().first()?, address - 0xA000),
            0xC000..=0xDFFF => {
                let bank = ((address - 0xC000) / WRAM_BANK_SIZE) as usize;
                (self.memory.wram_bank(bank)?, address % WRAM_BANK_SIZE)
            }
            0xE000..=0xFDFF => {
                let mirrored = self.achievement_slice(address - 0x2000)?;
                let room = (0xFE00 - address) as usize;
                return Some(&mirrored[..mirrored.len().min(room)]);
            }
            0xFF80..=0xFFFE => (self.memory.hram(), address - 0xFF80),
            0x1_0000..=0x1_5FFF => {
                let bank = 2 + ((address - 0x1_0000) / WRAM_BANK_SIZE) as usize;
                (self.memory.wram_bank(bank)?, address % WRAM_BANK_SIZE)
            }
            0x1_6000..=0x3_3FFF => {
                let offset = address - 0x1_6000;
                let bank = 1 + (offset / RAM_BANK_SIZE as u32) as usize;
                (
                    self.cartridge.ram_banks().get(bank)?,
                    offset % RAM_BANK_SIZE as u32,
                )
            }
            _ => return None,
        };
        bytes.get(offset as usize..)
    }

    fn wram_byte(&self, bank: usize, offset: u32) -> Option<u8> {
        self.memory
            .wram_bank(bank)
            .map(|bytes| bytes[offset as usize])
    }

    fn cart_ram_byte(&self, bank: usize, offset: u32) -> u8 {
        self.cartridge
            .ram_banks()
            .get(bank)
            .map_or(0, |bytes| bytes[offset as usize])
    }
}

#[cfg(test)]
mod tests {
    use crate::gb::{GameBoy, GbModel};
    use crate::{ReadMemory, WriteMemory};

    #[test]
    fn the_cpu_view_reads_without_side_effects() {
        let mut gb = GameBoy::new_empty(GbModel::Dmg);
        gb.memory.write_naive(0xC123, 0x42);
        gb.memory.write_naive(0xFF85, 0x17);
        assert_eq!(gb.achievement_peek(0xC123), Some(0x42));
        assert_eq!(gb.achievement_peek(0xE123), Some(0x42));
        assert_eq!(gb.achievement_peek(0xFF85), Some(0x17));
        assert_eq!(gb.memory.read_naive(0xC123), 0x42);
    }

    #[test]
    fn gbc_wram_banks_sit_above_the_cpu_view() {
        let mut gb = GameBoy::new_empty(GbModel::Cgb);
        gb.memory.wram_bank_mut(1).unwrap()[0x10] = 1;
        gb.memory.wram_bank_mut(2).unwrap()[0x10] = 2;
        gb.memory.wram_bank_mut(7).unwrap()[0xFFF] = 7;
        gb.memory.set_svbk(3);
        assert_eq!(gb.achievement_peek(0xD010), Some(1));
        assert_eq!(gb.achievement_peek(0x1_0010), Some(2));
        assert_eq!(gb.achievement_peek(0x1_5FFF), Some(7));
    }

    #[test]
    fn dmg_reads_zero_where_gbc_has_wram_banks() {
        let gb = GameBoy::new_empty(GbModel::Dmg);
        assert_eq!(gb.achievement_peek(0x1_0000), Some(0));
        assert_eq!(gb.achievement_peek(0x3_3FFF), Some(0));
        assert_eq!(gb.achievement_peek(0x3_4000), None);
    }

    #[test]
    fn a_long_read_crosses_from_one_bank_to_the_next() {
        let mut gb = GameBoy::new_empty(GbModel::Cgb);
        gb.memory.wram_bank_mut(0).unwrap()[0xFFE] = 1;
        gb.memory.wram_bank_mut(0).unwrap()[0xFFF] = 2;
        gb.memory.wram_bank_mut(1).unwrap()[0] = 3;
        gb.memory.wram_bank_mut(1).unwrap()[1] = 4;
        let mut buffer = [0; 4];
        assert_eq!(gb.achievement_read(0xCFFE, &mut buffer), 4);
        assert_eq!(buffer, [1, 2, 3, 4]);
    }

    #[test]
    fn every_byte_reads_the_same_by_slice_and_by_peek() {
        let mut gb = GameBoy::new_empty(GbModel::Cgb);
        for bank in 0..8 {
            for (i, byte) in gb
                .memory
                .wram_bank_mut(bank)
                .unwrap()
                .iter_mut()
                .enumerate()
            {
                *byte = (i as u8).wrapping_mul(7).wrapping_add(bank as u8);
            }
        }
        for address in (0..0x3_4000).step_by(3) {
            let mut one = [0];
            let read = gb.achievement_read(address, &mut one);
            assert_eq!(read, 1, "{address:x}");
            assert_eq!(Some(one[0]), gb.achievement_peek(address), "{address:x}");
        }
    }

    #[test]
    fn a_read_stops_at_the_end_of_the_map() {
        let gb = GameBoy::new_empty(GbModel::Dmg);
        let mut buffer = [0xAA; 4];
        assert_eq!(gb.achievement_read(0x3_3FFE, &mut buffer), 2);
    }
}

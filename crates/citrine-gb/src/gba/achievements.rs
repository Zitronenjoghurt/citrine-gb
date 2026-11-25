use super::machine::Gba;
use crate::achievement_map::{Region, read};

const IWRAM: u32 = 0x0;
const EWRAM: u32 = 0x8000;
const SAVE: u32 = 0x4_8000;
const SAVE_LEN: u32 = 0x1_0000;

impl Gba {
    pub fn achievement_read(&self, address: u32, buffer: &mut [u8]) -> usize {
        let bus = self.bus();
        let map = [
            Region {
                start: IWRAM,
                len: EWRAM - IWRAM,
                bytes: bus.iwram(),
            },
            Region {
                start: EWRAM,
                len: SAVE - EWRAM,
                bytes: bus.ewram(),
            },
            Region {
                start: SAVE,
                len: SAVE_LEN,
                bytes: bus.backup().bytes(),
            },
        ];
        read(&map, address, buffer)
    }
}

#[cfg(test)]
mod tests {
    use crate::gba::machine::Gba;
    use crate::gba::save::SaveType;

    fn peek(gba: &Gba, address: u32) -> u8 {
        let mut byte = [0xAA];
        assert_eq!(gba.achievement_read(address, &mut byte), 1, "{address:x}");
        byte[0]
    }

    fn gba() -> Gba {
        Gba::new(&[0; 0x200])
    }

    #[test]
    fn iwram_sits_at_zero_and_ewram_after_it() {
        let mut gba = gba();
        gba.bus_mut().write8(0x0300_0010, 0x11);
        gba.bus_mut().write8(0x0300_7FFF, 0x12);
        gba.bus_mut().write8(0x0200_0020, 0x21);
        gba.bus_mut().write8(0x0203_FFFF, 0x22);
        assert_eq!(peek(&gba, 0x10), 0x11);
        assert_eq!(peek(&gba, 0x7FFF), 0x12);
        assert_eq!(peek(&gba, 0x8020), 0x21);
        assert_eq!(peek(&gba, 0x4_7FFF), 0x22);
    }

    #[test]
    fn sram_sits_after_ewram() {
        let mut gba = gba();
        *gba.bus_mut().backup_mut() = crate::gba::save::Backup::new(SaveType::Sram32K);
        gba.bus_mut().write8(0x0E00_0005, 0x55);
        gba.bus_mut().write8(0x0E00_7FFF, 0x56);
        assert_eq!(peek(&gba, 0x4_8005), 0x55);
        assert_eq!(peek(&gba, 0x4_FFFF), 0x56);
        assert_eq!(peek(&gba, 0x5_0000), 0);
        assert_eq!(peek(&gba, 0x5_7FFF), 0);
    }

    #[test]
    fn flash_shows_its_first_64k() {
        let mut gba = gba();
        let mut saved = vec![0xFF; 0x2_0000];
        saved[0x1234] = 0x77;
        saved[0x1_1234] = 0x88;
        *gba.bus_mut().backup_mut() = crate::gba::save::Backup::new(SaveType::Flash128K);
        gba.bus_mut().backup_mut().load(&saved);
        assert_eq!(peek(&gba, 0x4_8000 + 0x1234), 0x77);
        assert_eq!(peek(&gba, 0x5_7FFF), 0xFF);
    }

    #[test]
    fn the_map_ends_after_save_ram() {
        let gba = gba();
        let mut buffer = [0xAA; 4];
        assert_eq!(gba.achievement_read(0x5_7FFE, &mut buffer), 2);
        assert_eq!(gba.achievement_read(0x5_8000, &mut buffer), 0);
    }
}

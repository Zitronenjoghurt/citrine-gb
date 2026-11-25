mod detect;
mod eeprom;
mod flash;

use eeprom::{Eeprom, EepromSize};
use flash::{Flash, FlashChip};

const SRAM_SMALL: usize = 0x8000;
const SRAM_LARGE: usize = 0x1_0000;
const ERASED: u8 = 0xFF;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SaveType {
    Auto,
    Sram32K,
    Sram64K,
    Flash64K,
    Flash128K,
    Eeprom,
    Eeprom512,
    Eeprom8K,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
enum Chip {
    Auto(Vec<u8>),
    Sram(Vec<u8>),
    Flash(Flash),
    Eeprom(Eeprom),
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Backup {
    chip: Chip,
    #[cfg_attr(
        feature = "serde",
        serde(serialize_with = "crate::utils::unsaved::serialize")
    )]
    dirty: bool,
}

impl Backup {
    pub fn new(kind: SaveType) -> Self {
        let chip = match kind {
            SaveType::Auto => Chip::Auto(Vec::new()),
            SaveType::Sram32K => Chip::Sram(vec![ERASED; SRAM_SMALL]),
            SaveType::Sram64K => Chip::Sram(vec![ERASED; SRAM_LARGE]),
            SaveType::Flash64K => Chip::Flash(Flash::new(FlashChip::Panasonic)),
            SaveType::Flash128K => Chip::Flash(Flash::new(FlashChip::Sanyo)),
            SaveType::Eeprom => Chip::Eeprom(Eeprom::new(None)),
            SaveType::Eeprom512 => Chip::Eeprom(Eeprom::new(Some(EepromSize::Small))),
            SaveType::Eeprom8K => Chip::Eeprom(Eeprom::new(Some(EepromSize::Large))),
        };
        Self { chip, dirty: false }
    }

    pub fn for_rom(rom: &[u8]) -> Self {
        Self::new(SaveType::detect(rom))
    }

    pub fn kind(&self) -> SaveType {
        match &self.chip {
            Chip::Auto(_) => SaveType::Auto,
            Chip::Sram(data) if data.len() == SRAM_SMALL => SaveType::Sram32K,
            Chip::Sram(_) => SaveType::Sram64K,
            Chip::Flash(flash) if flash.chip().is_large() => SaveType::Flash128K,
            Chip::Flash(_) => SaveType::Flash64K,
            Chip::Eeprom(eeprom) => match eeprom.size() {
                None => SaveType::Eeprom,
                Some(EepromSize::Small) => SaveType::Eeprom512,
                Some(EepromSize::Large) => SaveType::Eeprom8K,
            },
        }
    }

    pub fn bytes(&self) -> &[u8] {
        match &self.chip {
            Chip::Auto(data) | Chip::Sram(data) => data,
            Chip::Flash(flash) => flash.bytes(),
            Chip::Eeprom(eeprom) => eeprom.bytes(),
        }
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    pub fn load(&mut self, bytes: &[u8]) {
        match &mut self.chip {
            Chip::Auto(data) => *data = bytes.to_vec(),
            Chip::Sram(data) => {
                let size = if bytes.len() > SRAM_SMALL {
                    SRAM_LARGE
                } else {
                    data.len()
                };
                *data = fitted(bytes, size);
            }
            Chip::Flash(flash) => {
                if bytes.len() > flash.bytes().len() {
                    *flash = Flash::new(FlashChip::Sanyo);
                }
                flash.load(bytes);
            }
            Chip::Eeprom(eeprom) => eeprom.load(bytes),
        }
    }

    pub fn read8(&self, offset: usize) -> u8 {
        match &self.chip {
            Chip::Sram(data) => data[offset % data.len()],
            Chip::Flash(flash) => flash.read(offset),
            Chip::Auto(data) if !data.is_empty() => data[offset % data.len()],
            Chip::Auto(_) | Chip::Eeprom(_) => ERASED,
        }
    }

    pub fn write8(&mut self, offset: usize, value: u8) {
        if let Chip::Auto(_) = self.chip {
            let kind = if flash::starts_command(offset, value) {
                SaveType::Flash64K
            } else {
                SaveType::Sram32K
            };
            self.settle(kind);
        }
        match &mut self.chip {
            Chip::Sram(data) => {
                let len = data.len();
                data[offset % len] = value;
                self.dirty = true;
            }
            Chip::Flash(flash) => self.dirty |= flash.write(offset, value),
            Chip::Auto(_) | Chip::Eeprom(_) => {}
        }
    }

    pub fn uses_eeprom(&self) -> bool {
        matches!(self.chip, Chip::Eeprom(_))
    }

    pub fn read_eeprom(&self) -> u16 {
        match &self.chip {
            Chip::Eeprom(eeprom) => eeprom.read(),
            _ => 0,
        }
    }

    pub fn write_eeprom(&mut self, value: u16) {
        if let Chip::Auto(_) = self.chip {
            self.settle(SaveType::Eeprom);
        }
        if let Chip::Eeprom(eeprom) = &mut self.chip {
            self.dirty |= eeprom.write(value & 1 == 1);
        }
    }

    pub fn eeprom_dma(&mut self, units: u32) {
        if let Chip::Auto(_) = self.chip {
            self.settle(SaveType::Eeprom);
        }
        if let Chip::Eeprom(eeprom) = &mut self.chip {
            eeprom.start_transfer(units);
        }
    }

    fn settle(&mut self, kind: SaveType) {
        if let Chip::Auto(pending) = &mut self.chip {
            let pending = std::mem::take(pending);
            *self = Self::new(kind);
            if !pending.is_empty() {
                self.load(&pending);
            }
        }
    }
}

fn fitted(bytes: &[u8], size: usize) -> Vec<u8> {
    let mut data = vec![ERASED; size];
    let len = bytes.len().min(size);
    data[..len].copy_from_slice(&bytes[..len]);
    data
}

#[cfg(test)]
mod tests {
    use super::{Backup, SaveType};
    use crate::gba::bus::Bus;

    const SAVE: u32 = 0x0E00_0000;

    const EEPROM: u32 = 0x0D00_0000;

    const BUFFER: u32 = 0x0200_0000;

    const READ_BACK: u32 = 0x0200_1000;

    const DMA3: u32 = 0x0400_00D4;

    fn rom_with(library: &[u8]) -> Vec<u8> {
        let mut rom = vec![0; 0x400];
        rom[0x200..0x200 + library.len()].copy_from_slice(library);
        rom
    }

    fn flash_command(bus: &mut Bus, command: u8) {
        bus.write8(SAVE + 0x5555, 0xAA);
        bus.write8(SAVE + 0x2AAA, 0x55);
        bus.write8(SAVE + 0x5555, command);
    }

    fn flash_write(bus: &mut Bus, addr: u32, value: u8) {
        flash_command(bus, 0xA0);
        bus.write8(addr, value);
    }

    fn flash_erase(bus: &mut Bus, addr: u32, command: u8) {
        flash_command(bus, 0x80);
        bus.write8(SAVE + 0x5555, 0xAA);
        bus.write8(SAVE + 0x2AAA, 0x55);
        bus.write8(addr, command);
    }

    fn dma3(bus: &mut Bus, source: u32, dest: u32, units: usize) {
        bus.write32(DMA3, source);
        bus.write32(DMA3 + 4, dest);
        bus.write16(DMA3 + 8, units as u16);
        bus.write16(DMA3 + 10, 0x8000);
        bus.run_dma();
    }

    fn send_bits(bus: &mut Bus, dest: u32, bits: &[u16]) {
        for (i, &bit) in bits.iter().enumerate() {
            bus.write16(BUFFER + 2 * i as u32, bit);
        }
        dma3(bus, BUFFER, dest, bits.len());
    }

    fn push_bits(bits: &mut Vec<u16>, value: u64, count: u32) {
        bits.extend((0..count).rev().map(|i| (value >> i & 1) as u16));
    }

    fn eeprom_write(bus: &mut Bus, address_bits: u32, block: u64, value: u64) {
        let mut bits = vec![1, 0];
        push_bits(&mut bits, block, address_bits);
        push_bits(&mut bits, value, 64);
        bits.push(0);
        send_bits(bus, EEPROM, &bits);
    }

    fn eeprom_read(bus: &mut Bus, address_bits: u32, block: u64) -> u64 {
        let mut bits = vec![1, 1];
        push_bits(&mut bits, block, address_bits);
        bits.push(0);
        send_bits(bus, EEPROM, &bits);
        dma3(bus, EEPROM, READ_BACK, 68);
        (0..68)
            .map(|i| bus.read16(READ_BACK + 2 * i) & 1)
            .skip(4)
            .fold(0, |value, bit| value << 1 | u64::from(bit))
    }

    #[test]
    fn library_strings_pick_the_save_type() {
        let cases: [(&[u8], SaveType); 6] = [
            (b"SRAM_V113", SaveType::Sram32K),
            (b"SRAM_F_V102", SaveType::Sram32K),
            (b"FLASH_V126", SaveType::Flash64K),
            (b"FLASH512_V131", SaveType::Flash64K),
            (b"FLASH1M_V103", SaveType::Flash128K),
            (b"EEPROM_V124", SaveType::Eeprom),
        ];
        for (library, kind) in cases {
            assert_eq!(SaveType::detect(&rom_with(library)), kind, "{library:?}");
        }
    }

    #[test]
    fn flash_strings_win_over_sram_and_strings_must_be_word_aligned() {
        let mut rom = rom_with(b"SRAM_V113");
        rom[0x300..0x30A].copy_from_slice(b"FLASH1M_V1");
        assert_eq!(SaveType::detect(&rom), SaveType::Flash128K);
        let mut rom = vec![0; 0x400];
        rom[0x201..0x20A].copy_from_slice(b"SRAM_V113");
        assert_eq!(SaveType::detect(&rom), SaveType::Auto);
    }

    #[test]
    fn a_rom_without_a_string_settles_on_its_first_access() {
        let mut bus = Bus::new(&rom_with(b""));
        assert_eq!(bus.backup().kind(), SaveType::Auto);
        assert_eq!(bus.read8(SAVE), 0xFF);
        assert!(bus.backup().bytes().is_empty());
        bus.write8(SAVE + 3, 0x42);
        assert_eq!(bus.backup().kind(), SaveType::Sram32K);
        assert_eq!(bus.read8(SAVE + 3), 0x42);

        let mut bus = Bus::new(&rom_with(b""));
        flash_command(&mut bus, 0x90);
        assert_eq!(bus.backup().kind(), SaveType::Flash64K);
        assert_eq!(bus.read8(SAVE), 0x32);
    }

    #[test]
    fn sram_32k_mirrors_across_the_save_region() {
        let mut bus = Bus::new(&rom_with(b"SRAM_V113"));
        bus.write8(SAVE + 0x10, 0x5A);
        assert_eq!(bus.read8(SAVE + 0x8010), 0x5A);
        assert_eq!(bus.read8(0x0F00_0010), 0x5A);
        assert_eq!(bus.read16(SAVE + 0x10), 0x5A5A);
        assert_eq!(bus.backup().bytes().len(), 0x8000);
    }

    #[test]
    fn sram_64k_comes_from_a_64k_file_and_does_not_mirror_at_32k() {
        let mut bus = Bus::new(&rom_with(b"SRAM_V113"));
        let mut file = vec![0xFF; 0x1_0000];
        file[0x8010] = 0x77;
        bus.backup_mut().load(&file);
        assert_eq!(bus.backup().kind(), SaveType::Sram64K);
        assert_eq!(bus.read8(SAVE + 0x8010), 0x77);
        assert_eq!(bus.read8(SAVE + 0x10), 0xFF);
        assert_eq!(bus.read8(SAVE + 0x1_8010), 0x77);
    }

    #[test]
    fn flash_writes_a_byte_only_after_the_write_command() {
        let mut bus = Bus::new(&rom_with(b"FLASH_V126"));
        bus.write8(SAVE + 0x100, 0x12);
        assert_eq!(bus.read8(SAVE + 0x100), 0xFF);
        assert!(!bus.backup().dirty());
        flash_write(&mut bus, SAVE + 0x100, 0x34);
        assert_eq!(bus.read8(SAVE + 0x100), 0x34);
        assert!(bus.backup().dirty());
        bus.write8(SAVE + 0x101, 0x56);
        assert_eq!(bus.read8(SAVE + 0x101), 0xFF);
    }

    #[test]
    fn flash_sector_erase_clears_only_its_4k() {
        let mut bus = Bus::new(&rom_with(b"FLASH512_V131"));
        for addr in [0x0FFF, 0x1000, 0x1FFF, 0x2000] {
            flash_write(&mut bus, SAVE + addr, 0x00);
        }
        flash_erase(&mut bus, SAVE + 0x1000, 0x30);
        let after: Vec<u8> = [0x0FFF, 0x1000, 0x1FFF, 0x2000]
            .iter()
            .map(|&addr| bus.read8(SAVE + addr))
            .collect();
        assert_eq!(after, [0x00, 0xFF, 0xFF, 0x00]);
    }

    #[test]
    fn flash_chip_erase_clears_everything() {
        let mut bus = Bus::new(&rom_with(b"FLASH1M_V103"));
        flash_write(&mut bus, SAVE, 0x00);
        flash_command(&mut bus, 0xB0);
        bus.write8(SAVE, 1);
        flash_write(&mut bus, SAVE + 0xFFFF, 0x00);
        flash_erase(&mut bus, SAVE + 0x5555, 0x10);
        assert!(bus.backup().bytes().iter().all(|&byte| byte == 0xFF));
    }

    #[test]
    fn flash_128k_switches_banks_and_64k_ignores_the_switch() {
        let mut bus = Bus::new(&rom_with(b"FLASH1M_V103"));
        flash_write(&mut bus, SAVE + 0x10, 0xA0);
        flash_command(&mut bus, 0xB0);
        bus.write8(SAVE, 1);
        assert_eq!(bus.read8(SAVE + 0x10), 0xFF);
        flash_write(&mut bus, SAVE + 0x10, 0xB1);
        flash_command(&mut bus, 0xB0);
        bus.write8(SAVE, 0);
        assert_eq!(bus.read8(SAVE + 0x10), 0xA0);
        assert_eq!(bus.backup().bytes()[0x1_0010], 0xB1);

        let mut bus = Bus::new(&rom_with(b"FLASH_V126"));
        flash_write(&mut bus, SAVE + 0x10, 0xA0);
        flash_command(&mut bus, 0xB0);
        bus.write8(SAVE, 1);
        assert_eq!(bus.read8(SAVE + 0x10), 0xA0);
        assert_eq!(bus.read8(SAVE), 0xFF);
    }

    #[test]
    fn eeprom_512_reads_and_writes_blocks_through_dma3() {
        let mut bus = Bus::new(&rom_with(b"EEPROM_V124"));
        eeprom_write(&mut bus, 6, 0x3F, 0x0123_4567_89AB_CDEF);
        assert_eq!(bus.backup().kind(), SaveType::Eeprom512);
        assert_eq!(bus.read16(0x0DFF_FF00) & 1, 1);
        assert_eq!(eeprom_read(&mut bus, 6, 0x3F), 0x0123_4567_89AB_CDEF);
        assert_eq!(eeprom_read(&mut bus, 6, 0), u64::MAX);
        assert_eq!(bus.backup().bytes().len(), 0x200);
        assert_eq!(
            bus.backup().bytes()[0x1F8..],
            [0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF]
        );
    }

    #[test]
    fn eeprom_8k_uses_14_bit_addresses() {
        let mut bus = Bus::new(&rom_with(b"EEPROM_V124"));
        eeprom_write(&mut bus, 14, 0x3FF, 0xFEDC_BA98_7654_3210);
        eeprom_write(&mut bus, 14, 0x001, 0x1111_2222_3333_4444);
        assert_eq!(bus.backup().kind(), SaveType::Eeprom8K);
        assert_eq!(eeprom_read(&mut bus, 14, 0x3FF), 0xFEDC_BA98_7654_3210);
        assert_eq!(eeprom_read(&mut bus, 14, 0x001), 0x1111_2222_3333_4444);
        assert_eq!(bus.backup().bytes().len(), 0x2000);
        assert_eq!(bus.backup().bytes()[0x1FF8], 0xFE);
    }

    #[test]
    fn an_eeprom_dma_settles_a_rom_without_a_string() {
        let mut bus = Bus::new(&rom_with(b""));
        eeprom_write(&mut bus, 14, 2, 42);
        assert_eq!(bus.backup().kind(), SaveType::Eeprom8K);
        assert_eq!(eeprom_read(&mut bus, 14, 2), 42);
    }

    #[test]
    fn a_32m_rom_keeps_eeprom_to_its_last_256_bytes() {
        let mut rom = vec![0; 0x200_0000];
        rom[0x200..0x20B].copy_from_slice(b"EEPROM_V124");
        rom[0x0100_0000..0x0100_0002].copy_from_slice(&[0x34, 0x12]);
        let mut bus = Bus::new(&rom);
        assert_eq!(bus.read16(EEPROM), 0x1234);
        let mut bits = vec![1, 0];
        push_bits(&mut bits, 5, 6);
        push_bits(&mut bits, 99, 64);
        bits.push(0);
        send_bits(&mut bus, 0x0DFF_FF00, &bits);
        assert_eq!(bus.backup().bytes()[5 * 8 + 7], 99);
    }

    #[test]
    fn saves_round_trip_with_the_usual_file_sizes() {
        let cases: [(&[u8], usize); 5] = [
            (b"SRAM_V113", 0x8000),
            (b"FLASH512_V131", 0x1_0000),
            (b"FLASH1M_V103", 0x2_0000),
            (b"EEPROM_V124", 0x200),
            (b"EEPROM_V124", 0x2000),
        ];
        for (library, size) in cases {
            let rom = rom_with(library);
            let mut bus = Bus::new(&rom);
            match (library[0], size) {
                (b'S', _) => bus.write8(SAVE + 0x7FFF, 0x99),
                (b'F', _) => {
                    flash_command(&mut bus, 0xB0);
                    bus.write8(SAVE, 1);
                    flash_write(&mut bus, SAVE + 0xFFFF, 0x99);
                }
                (_, 0x200) => eeprom_write(&mut bus, 6, 0x3F, 0x99),
                _ => eeprom_write(&mut bus, 14, 0x3FF, 0x99),
            }
            assert!(bus.backup().dirty());
            let file = bus.backup().bytes().to_vec();
            assert_eq!(file.len(), size, "{library:?}");
            assert_eq!(file[size - 1], 0x99, "{library:?}");

            let mut fresh = Bus::new(&rom);
            fresh.backup_mut().load(&file);
            assert_eq!(fresh.backup().bytes(), file.as_slice(), "{library:?}");
            assert!(!fresh.backup().dirty());
        }
    }

    #[test]
    fn a_file_loaded_before_the_first_access_survives_it() {
        let mut backup = Backup::new(SaveType::Auto);
        let file: Vec<u8> = (0..0x8000).map(|i| i as u8).collect();
        backup.load(&file);
        assert_eq!(backup.bytes(), file.as_slice());
        backup.write8(0, 0xEE);
        assert_eq!(backup.kind(), SaveType::Sram32K);
        assert_eq!(backup.read8(1), 1);
        assert_eq!(backup.read8(0), 0xEE);
    }

    #[test]
    fn a_file_loaded_before_the_first_access_reads_back_before_a_write() {
        let mut backup = Backup::new(SaveType::Auto);
        let file: Vec<u8> = (0..0x1_0000).map(|i| (i / 3) as u8).collect();
        backup.load(&file);
        assert_eq!(backup.read8(0x9001), file[0x9001]);
        assert_eq!(backup.kind(), SaveType::Auto);
        backup.write8(0x9002, 0xEE);
        assert_eq!(backup.kind(), SaveType::Sram64K);
        assert_eq!(backup.read8(0x9001), file[0x9001]);
    }

    #[test]
    fn an_eeprom_file_sets_the_size_until_a_dma_says_otherwise() {
        let mut backup = Backup::new(SaveType::Eeprom);
        backup.load(&[0x11; 0x2000]);
        assert_eq!(backup.kind(), SaveType::Eeprom8K);
        backup.eeprom_dma(73);
        assert_eq!(backup.kind(), SaveType::Eeprom512);
        assert_eq!(backup.bytes(), &[0x11; 0x200]);
    }

    #[test]
    fn the_dirty_flag_clears_and_reads_leave_it_clear() {
        let mut bus = Bus::new(&rom_with(b"SRAM_V113"));
        bus.write8(SAVE, 1);
        assert!(bus.backup().dirty());
        bus.backup_mut().clear_dirty();
        bus.read8(SAVE);
        assert!(!bus.backup().dirty());
    }
}

use super::bus::Buses;
use super::card::{KeyTable, decrypt_secure_area};
use super::cp15::Fetch;
use crate::gba::cpu::{Arm9, Cpu as Arm7, CpuState};
use crate::gba::wait::{Access, Width};

const HEADER_BYTES: usize = 0x170;
const MIN_ROM_BYTES: usize = 0x200;
const GAME_CODE: usize = 0x0C;
const ARM9_BINARY: usize = 0x20;
const ARM7_BINARY: usize = 0x30;
const SECURE_AREA_CRC: usize = 0x6C;
const LOGO: usize = 0xC0;
const LOGO_BYTES: usize = 0x9C;
const HEADER_CRC: usize = 0x15E;
const BIOS_LOGO: usize = 0x20;
const SECURE_AREA_START: u32 = 0x4000;
const SECURE_AREA_END: u32 = 0x8000;
const SECURE_AREA_BYTES: usize = 0x800;
const SECURE_AREA_ID: &[u8; 8] = b"encryObj";
const SECURE_AREA_BOOTED: u32 = 0xE7FF_DEFF;
const HEADER_COPY: u32 = 0x027F_FE00;
const BOOT_INFO: [u32; 2] = [0x027F_F800, 0x027F_FC00];
const ARM7_BIOS_CRC: u16 = 0x5835;
const ARM7_BIOS_CRC_AT: [u32; 2] = [0x027F_F850, 0x027F_FC10];
const GBA_HEADER_CRC_AT: u32 = 0x027F_FC30;
const BOOT_INDICATOR_AT: u32 = 0x027F_FC40;
const CART_BOOT: u32 = 1;
const DOWNLOAD_BOOT: u32 = 2;
const PARENT_AT: u32 = 0x027F_FC42;
const FIRMWARE_INFO: u32 = 0x027F_F864;
const SETTINGS_OFFSET_AT: u32 = 0x027F_F868;
const DATA_CRC_AT: u32 = 0x027F_F874;
const CODE_CRC_AT: u32 = 0x027F_F876;
const USER_SETTINGS_AT: u32 = 0x027F_FC80;
const USER_SETTINGS_COPY: usize = 0x70;
const FIRMWARE_CODE_CRC: usize = 0x04;
const FIRMWARE_SETTINGS_OFFSET: usize = 0x20;
const FIRMWARE_DATA_CRC: usize = 0x26;
const WRAMCNT: u32 = 0x0400_0247;
const POSTFLG: u32 = 0x0400_0300;
const POWCNT1: u32 = 0x0400_0304;
const EXMEMCNT: u32 = 0x0400_0204;
const AUXSPICNT: u32 = 0x0400_01A0;
const RCNT: u32 = 0x0400_0134;
const WIFIWAITCNT: u32 = 0x0400_0206;
const BIOSPROT: u32 = 0x0400_0308;
const SOUNDBIAS: u32 = 0x0400_0504;
const ALL_WRAM_TO_ARM7: u32 = 3;
const BOOTED: u32 = 1;
const POWER_AFTER_BOOT: u32 = 0x820F;
const EXMEM_AFTER_BOOT: u32 = 0x4000;
const AUX_AFTER_BOOT: u32 = 0x8000;
const RCNT_AFTER_BOOT: u32 = 0x8000;
const WIFI_WAIT_AFTER_BOOT: u32 = 0x0030;
const BIOSPROT_AFTER_BOOT: u32 = 0x1204;
const BIAS_AFTER_BOOT: u32 = 0x0200;
const CPSR_SYSTEM: u32 = 0x1F;
const ARM9_STACKS: Stacks = Stacks {
    system: 0x0300_2F7C,
    irq: 0x0300_3F80,
    supervisor: 0x0300_3FC0,
};
const ARM7_STACKS: Stacks = Stacks {
    system: 0x0380_FD80,
    irq: 0x0380_FF80,
    supervisor: 0x0380_FFC0,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binary {
    pub rom_offset: u32,
    pub entry: u32,
    pub ram_address: u32,
    pub size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub title: String,
    pub game_code: u32,
    pub arm9: Binary,
    pub arm7: Binary,
}

struct Stacks {
    system: u32,
    irq: u32,
    supervisor: u32,
}

pub struct Boot<'a> {
    pub rom: &'a [u8],
    pub key_table: Option<&'a KeyTable>,
    pub free_bios: bool,
    pub firmware: &'a [u8],
    pub parent: Option<&'a [u8]>,
}

impl Binary {
    fn parse(rom: &[u8], at: usize) -> Self {
        Self {
            rom_offset: read32(rom, at),
            entry: read32(rom, at + 4),
            ram_address: read32(rom, at + 8),
            size: read32(rom, at + 12),
        }
    }

    fn fits(self, rom: &[u8]) -> bool {
        (self.rom_offset as usize)
            .checked_add(self.size as usize)
            .is_some_and(|end| end <= rom.len())
    }
}

impl Header {
    pub fn parse(rom: &[u8]) -> Option<Self> {
        if rom.len() < MIN_ROM_BYTES {
            return None;
        }
        let header = Self {
            title: rom[..12]
                .iter()
                .take_while(|&&byte| byte != 0)
                .map(|&byte| char::from(byte))
                .collect::<String>()
                .trim()
                .to_string(),
            game_code: read32(rom, GAME_CODE),
            arm9: Binary::parse(rom, ARM9_BINARY),
            arm7: Binary::parse(rom, ARM7_BINARY),
        };
        (header.arm9.fits(rom) && header.arm7.fits(rom)).then_some(header)
    }
}

impl Boot<'_> {
    pub fn run(&self, header: &Header, buses: &mut Buses) -> (Arm9, Arm7) {
        buses.arm9().write(WRAMCNT, Width::Byte, ALL_WRAM_TO_ARM7);
        if self.free_bios {
            buses.patch_arm9_bios(BIOS_LOGO, &self.rom[LOGO..LOGO + LOGO_BYTES]);
        }
        self.write_boot_info(buses);
        self.write_firmware_info(buses);
        let arm9_code = self.arm9_code(header);
        copy(buses, header.arm9.ram_address, &arm9_code, Side::Arm9);
        let arm7 = header.arm7;
        let arm7_code = &self.rom[arm7.rom_offset as usize..(arm7.rom_offset + arm7.size) as usize];
        copy(buses, arm7.ram_address, arm7_code, Side::Arm7);
        buses.boot_cp15();
        buses.cart_mut().direct_boot();
        for (addr, value, width) in [
            (POSTFLG, BOOTED, Width::Byte),
            (POWCNT1, POWER_AFTER_BOOT, Width::Half),
            (EXMEMCNT, EXMEM_AFTER_BOOT, Width::Half),
            (AUXSPICNT, AUX_AFTER_BOOT, Width::Half),
        ] {
            buses.arm9().write(addr, width, value);
        }
        for (addr, value, width) in [
            (POSTFLG, BOOTED, Width::Byte),
            (EXMEMCNT, EXMEM_AFTER_BOOT, Width::Half),
            (RCNT, RCNT_AFTER_BOOT, Width::Half),
            (WIFIWAITCNT, WIFI_WAIT_AFTER_BOOT, Width::Half),
            (BIOSPROT, BIOSPROT_AFTER_BOOT, Width::Half),
            (SOUNDBIAS, BIAS_AFTER_BOOT, Width::Half),
        ] {
            buses.arm7().write(addr, width, value);
        }
        let arm9_entry = header.arm9.entry;
        let pipeline = [0, 4].map(|step| {
            buses
                .arm9()
                .cpu_read(arm9_entry.wrapping_add(step), Width::Word, Fetch::Code)
        });
        let arm9 = Arm9::from_state(&entry_state(arm9_entry, &ARM9_STACKS, pipeline));
        let arm7_entry = header.arm7.entry;
        let pipeline = [0, 4].map(|step| {
            buses
                .arm7()
                .read(arm7_entry.wrapping_add(step), Width::Word)
        });
        let arm7 = Arm7::from_state(&entry_state(arm7_entry, &ARM7_STACKS, pipeline));
        (arm9, arm7)
    }

    fn write_boot_info(&self, buses: &mut Buses) {
        let chip_id = buses.cart().chip_id();
        let mut bus = buses.arm9();
        for offset in (0..HEADER_BYTES).step_by(4) {
            bus.write(
                HEADER_COPY + offset as u32,
                Width::Word,
                read32(self.rom, offset),
            );
        }
        let header_crc = u32::from(read16(self.rom, HEADER_CRC));
        let secure_crc = u32::from(read16(self.rom, SECURE_AREA_CRC));
        for base in BOOT_INFO {
            bus.write(base, Width::Word, chip_id);
            bus.write(base + 4, Width::Word, chip_id);
            bus.write(base + 8, Width::Half, header_crc);
            bus.write(base + 10, Width::Half, secure_crc);
        }
        for addr in ARM7_BIOS_CRC_AT {
            bus.write(addr, Width::Half, u32::from(ARM7_BIOS_CRC));
        }
        bus.write(GBA_HEADER_CRC_AT, Width::Half, 0xFFFF);
        match self.parent {
            Some(parent) => {
                bus.write(BOOT_INDICATOR_AT, Width::Half, DOWNLOAD_BOOT);
                for (index, pair) in parent.chunks(2).enumerate() {
                    let value =
                        u32::from(pair[0]) | u32::from(pair.get(1).copied().unwrap_or(0)) << 8;
                    bus.write(PARENT_AT + 2 * index as u32, Width::Half, value);
                }
            }
            None => bus.write(BOOT_INDICATOR_AT, Width::Half, CART_BOOT),
        }
    }

    fn write_firmware_info(&self, buses: &mut Buses) {
        let firmware = self.firmware;
        let settings = u32::from(read16(firmware, FIRMWARE_SETTINGS_OFFSET)) << 3;
        let mut bus = buses.arm9();
        bus.write(FIRMWARE_INFO, Width::Word, 0);
        bus.write(SETTINGS_OFFSET_AT, Width::Word, settings);
        bus.write(
            DATA_CRC_AT,
            Width::Half,
            u32::from(read16(firmware, FIRMWARE_DATA_CRC)),
        );
        bus.write(
            CODE_CRC_AT,
            Width::Half,
            u32::from(read16(firmware, FIRMWARE_CODE_CRC)),
        );
        let user = settings as usize;
        if let Some(bytes) = firmware.get(user..user + USER_SETTINGS_COPY) {
            for (index, chunk) in bytes.chunks(4).enumerate() {
                bus.write(
                    USER_SETTINGS_AT + 4 * index as u32,
                    Width::Word,
                    read32(chunk, 0),
                );
            }
        }
    }

    fn arm9_code(&self, header: &Header) -> Vec<u8> {
        let arm9 = header.arm9;
        let start = arm9.rom_offset as usize;
        let mut code = self.rom[start..start + arm9.size as usize].to_vec();
        if (SECURE_AREA_START..SECURE_AREA_END).contains(&arm9.rom_offset)
            && let Some(table) = self.key_table
            && code.len() >= SECURE_AREA_BYTES
        {
            let mut area = code[..SECURE_AREA_BYTES].to_vec();
            decrypt_secure_area(&mut area, table, header.game_code);
            if &area[..8] == SECURE_AREA_ID {
                area[..4].copy_from_slice(&SECURE_AREA_BOOTED.to_le_bytes());
                area[4..8].copy_from_slice(&SECURE_AREA_BOOTED.to_le_bytes());
                code[..SECURE_AREA_BYTES].copy_from_slice(&area);
            }
        }
        code
    }
}

#[derive(Clone, Copy)]
enum Side {
    Arm9,
    Arm7,
}

fn copy(buses: &mut Buses, ram_address: u32, code: &[u8], side: Side) {
    for (index, chunk) in code.chunks(4).enumerate() {
        let addr = ram_address.wrapping_add(4 * index as u32);
        let mut word = [0; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        let value = u32::from_le_bytes(word);
        match side {
            Side::Arm9 => buses.arm9().write(addr, Width::Word, value),
            Side::Arm7 => buses.arm7().write(addr, Width::Word, value),
        }
    }
}

fn entry_state(entry: u32, stacks: &Stacks, pipeline: [u32; 2]) -> CpuState {
    let mut state = CpuState {
        cpsr: CPSR_SYSTEM,
        ..CpuState::default()
    };
    state.r[12] = entry;
    state.r[13] = stacks.system;
    state.r[14] = entry;
    state.irq[0] = stacks.irq;
    state.svc[0] = stacks.supervisor;
    state.r[15] = entry.wrapping_add(8);
    state.pipeline = pipeline;
    state.next_fetch = Access::Sequential;
    state
}

fn read16(bytes: &[u8], at: usize) -> u16 {
    bytes
        .get(at..at + 2)
        .map_or(0, |bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read32(bytes: &[u8], at: usize) -> u32 {
    let mut word = [0; 4];
    if let Some(slice) = bytes.get(at..) {
        let len = slice.len().min(4);
        word[..len].copy_from_slice(&slice[..len]);
    }
    u32::from_le_bytes(word)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARM9_RAM: u32 = 0x0200_0000;
    const ARM7_RAM: u32 = 0x0380_0000;

    fn rom() -> Vec<u8> {
        let mut rom = vec![0; 0x1000];
        rom[..8].copy_from_slice(b"TESTROM\0");
        rom[GAME_CODE..GAME_CODE + 4].copy_from_slice(b"ATST");
        for (at, values) in [
            (ARM9_BINARY, [0x200, ARM9_RAM + 4, ARM9_RAM, 0x10]),
            (ARM7_BINARY, [0x400, ARM7_RAM, ARM7_RAM, 0x8]),
        ] {
            for (index, value) in values.into_iter().enumerate() {
                rom[at + 4 * index..at + 4 * index + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
        rom[0x200..0x210].copy_from_slice(&[0x11; 16]);
        rom[0x400..0x408].copy_from_slice(&[0x22; 8]);
        rom[HEADER_CRC..HEADER_CRC + 2].copy_from_slice(&0xBEEFu16.to_le_bytes());
        rom
    }

    fn boot(rom: &[u8]) -> (Buses, Arm9, Arm7) {
        let header = Header::parse(rom).unwrap();
        let mut buses = Buses::default();
        buses.load_arm9_bios(&[0; 0x1000]);
        let firmware = super::super::spi::firmware::Firmware::default();
        let (arm9, arm7) = Boot {
            rom,
            key_table: None,
            free_bios: true,
            firmware: firmware.bytes(),
            parent: None,
        }
        .run(&header, &mut buses);
        (buses, arm9, arm7)
    }

    #[test]
    fn the_header_names_both_binaries() {
        let header = Header::parse(&rom()).unwrap();
        assert_eq!(header.title, "TESTROM");
        assert_eq!(header.game_code, u32::from_le_bytes(*b"ATST"));
        assert_eq!(header.arm9.entry, ARM9_RAM + 4);
        assert_eq!(header.arm7.size, 8);
    }

    #[test]
    fn a_binary_past_the_end_of_the_rom_is_no_header() {
        let mut rom = rom();
        rom[ARM7_BINARY + 12..ARM7_BINARY + 16].copy_from_slice(&0x10_0000u32.to_le_bytes());
        assert_eq!(Header::parse(&rom), None);
        assert_eq!(Header::parse(&[0; 0x100]), None);
    }

    #[test]
    fn direct_boot_copies_both_binaries_and_the_header() {
        let (mut buses, _, _) = boot(&rom());
        assert_eq!(buses.arm9().read(ARM9_RAM + 12, Width::Word), 0x1111_1111);
        assert_eq!(buses.arm7().read(ARM7_RAM + 4, Width::Word), 0x2222_2222);
        assert_eq!(buses.arm9().read(HEADER_COPY, Width::Word), 0x5453_4554);
        assert_eq!(buses.arm9().read(0x027F_F808, Width::Half), 0xBEEF);
        assert_eq!(buses.arm9().read(0x027F_FC40, Width::Half), 1);
    }

    #[test]
    fn both_cpus_start_at_their_entry_in_system_mode() {
        let (_, arm9, arm7) = boot(&rom());
        assert_eq!(arm9.pc(), ARM9_RAM + 4);
        assert_eq!(arm9.pipeline(), [0x1111_1111; 2]);
        assert_eq!(arm9.reg(13), 0x0300_2F7C);
        assert_eq!(arm9.cpsr(), CPSR_SYSTEM);
        assert_eq!(arm7.pc(), ARM7_RAM);
        assert_eq!(arm7.reg(13), 0x0380_FD80);
        assert_eq!(arm7.reg(12), ARM7_RAM);
    }

    #[test]
    fn direct_boot_leaves_the_registers_a_bios_leaves() {
        let (mut buses, _, _) = boot(&rom());
        assert_eq!(buses.arm9().read(POSTFLG, Width::Byte), 1);
        assert_eq!(buses.arm7().read(POSTFLG, Width::Byte), 1);
        assert_eq!(buses.arm9().read(POWCNT1, Width::Half), 0x820F);
        assert_eq!(buses.arm7().read(0x0400_0241, Width::Byte), 3);
        assert_eq!(
            buses.cp15().control(),
            super::super::cp15::DIRECT_BOOT_CONTROL
        );
        assert!(buses.tcm().itcm_region().enabled);
        assert_eq!(buses.tcm().dtcm_region().base, 0x0300_0000);
    }

    #[test]
    fn freebios_gets_the_logo_from_the_header() {
        let mut rom = rom();
        rom[LOGO..LOGO + LOGO_BYTES].fill(0x5A);
        let (mut buses, _, _) = boot(&rom);
        assert_eq!(buses.arm9().read(0xFFFF_0020, Width::Word), 0x5A5A_5A5A);
        assert_eq!(
            buses
                .arm9()
                .read(0xFFFF_0020 + LOGO_BYTES as u32, Width::Byte),
            0
        );
    }

    #[test]
    fn the_firmware_user_settings_go_to_27ffc80() {
        let firmware = super::super::spi::firmware::Firmware::default();
        let offset = usize::from(read16(firmware.bytes(), FIRMWARE_SETTINGS_OFFSET)) << 3;
        let expected = read32(firmware.bytes(), offset);
        let (mut buses, _, _) = boot(&rom());
        assert_eq!(buses.arm9().read(USER_SETTINGS_AT, Width::Word), expected);
        assert_eq!(
            buses.arm9().read(SETTINGS_OFFSET_AT, Width::Word),
            offset as u32
        );
    }
}

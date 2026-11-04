#[cfg(feature = "debug")]
use crate::debug::code::{Address, BlockId};
#[cfg(feature = "debug")]
use crate::debug::disassembly::DisassemblySource;
use crate::error::{GbError, GbResult};
use crate::gb::cartridge::mbc::MbcInterface;
use crate::rom::Rom;
use crate::rom::header::RomHeader;
use crate::{ReadMemory, WriteMemory};
use std::sync::Arc;

mod mbc;

pub use mbc::camera::{CAMERA_HEIGHT, CAMERA_PIXELS, CAMERA_WIDTH};
pub use mbc::mbc3::{RTC_CYCLES_PER_SECOND, Rtc, RtcRegisters};

pub const ROM_BANK_SIZE: usize = 0x4000; // 16KiB
pub const RAM_BANK_SIZE: usize = 0x2000; // 8KiB

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Cartridge {
    pub header: RomHeader,
    pub has_rom_loaded: bool,
    has_battery: bool,
    sram_dirty: bool,
    mbc: mbc::Mbc,
    #[cfg_attr(feature = "serde", serde(skip, default))]
    rom: Arc<[[u8; ROM_BANK_SIZE]]>,
    #[cfg_attr(feature = "serde", serde(skip, default))]
    ram: Vec<[u8; RAM_BANK_SIZE]>,
}

impl Default for Cartridge {
    fn default() -> Self {
        Self {
            header: RomHeader::default(),
            has_rom_loaded: false,
            has_battery: false,
            sram_dirty: false,
            mbc: mbc::Mbc::None,
            rom: vec![[0; ROM_BANK_SIZE]; 2].into(),
            ram: vec![[0; RAM_BANK_SIZE]; 1],
        }
    }
}

impl Clone for Cartridge {
    fn clone(&self) -> Self {
        Self {
            header: self.header.clone(),
            has_rom_loaded: self.has_rom_loaded,
            has_battery: self.has_battery,
            sram_dirty: self.sram_dirty,
            mbc: self.mbc.clone(),
            rom: self.rom.clone(),
            ram: self.ram.clone(),
        }
    }

    fn clone_from(&mut self, source: &Self) {
        let Self {
            header,
            has_rom_loaded,
            has_battery,
            sram_dirty,
            mbc,
            rom,
            ram,
        } = self;
        header.clone_from(&source.header);
        *has_rom_loaded = source.has_rom_loaded;
        *has_battery = source.has_battery;
        *sram_dirty = source.sram_dirty;
        mbc.clone_from(&source.mbc);
        if !Arc::ptr_eq(rom, &source.rom) {
            *rom = source.rom.clone();
        }
        ram.clone_from(&source.ram);
    }
}

impl Cartridge {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn has_battery(&self) -> bool {
        self.has_battery
    }

    pub fn ram_banks(&self) -> &[[u8; RAM_BANK_SIZE]] {
        &self.ram
    }

    #[cfg(feature = "debug")]
    pub(crate) fn rom_banks(&self) -> usize {
        self.rom.len()
    }

    #[cfg(feature = "debug")]
    pub(crate) fn rom_bank(&self, bank: usize) -> Option<&[u8]> {
        self.rom.get(bank).map(|bytes| &bytes[..])
    }

    #[cfg(feature = "debug")]
    pub(crate) fn ram_bank_mut(&mut self, bank: usize) -> Option<&mut [u8]> {
        self.sram_dirty = true;
        self.ram.get_mut(bank).map(|bytes| &mut bytes[..])
    }

    #[cfg(feature = "debug")]
    pub(crate) fn ram_address(&self, addr: u16) -> Option<Address> {
        if self.mbc.on_read(addr).is_some() {
            return None;
        }
        let bank = self.mbc.ram_bank()?;
        Some(Address::at(
            BlockId::CartRam,
            bank as u16,
            u32::from(addr - 0xA000),
        ))
    }

    #[cfg(feature = "persistence")]
    pub(crate) fn restore_storage(&mut self, like: &Self, ram: &[u8]) {
        self.rom = like.rom.clone();
        self.ram = vec![[0; RAM_BANK_SIZE]; like.ram.len()];
        for (bank, chunk) in self.ram.iter_mut().zip(ram.chunks(RAM_BANK_SIZE)) {
            bank[..chunk.len()].copy_from_slice(chunk);
        }
    }

    pub fn rom_bytes(&self) -> Vec<u8> {
        self.rom.concat()
    }

    #[cfg(feature = "persistence")]
    pub fn restore_rom(&mut self, rom: &Rom) -> GbResult<()> {
        let header = rom.header()?;
        let rom_banks = header.rom_banks.max(2);

        let mut banks = rom_banks_of(rom, rom_banks)?;
        banks.resize(rom_banks, [0; ROM_BANK_SIZE]);
        self.rom = banks.into();
        self.ram = vec![[0; RAM_BANK_SIZE]; header.ram_banks.max(1)];
        Ok(())
    }

    pub fn load_rom(&mut self, rom: &Rom) -> GbResult<()> {
        let header = rom.header()?;
        let rom_banks = header.rom_banks.max(2);
        let ram_banks = header.ram_banks.max(1);
        self.mbc = mbc::Mbc::try_from(&header)?;

        let mut banks = rom_banks_of(rom, rom_banks)?;
        self.mbc.set_multicart(is_mbc1_multicart(&banks));

        self.has_battery = header
            .cartridge_type
            .map(|ct| ct.has_battery())
            .unwrap_or(false);
        self.header = header;
        self.has_rom_loaded = true;
        banks.resize(rom_banks, [0; ROM_BANK_SIZE]);
        self.rom = banks.into();
        self.ram = vec![[0; RAM_BANK_SIZE]; ram_banks];

        Ok(())
    }

    pub fn soft_reset(&mut self) {
        self.mbc.soft_reset();
        if !self.has_battery {
            self.ram.iter_mut().for_each(|bank| bank.fill(0));
        }
    }

    pub fn cycle(&mut self) {
        if self.mbc.cycle(&mut self.ram) {
            self.sram_dirty = true;
        }
    }

    fn rom_byte(&self, offset: usize) -> u8 {
        let bank = offset / ROM_BANK_SIZE;
        let bank = if bank < self.rom.len() {
            bank
        } else {
            bank % self.rom.len()
        };
        self.rom[bank][offset % ROM_BANK_SIZE]
    }

    fn ram_index(&self, offset: usize) -> (usize, usize) {
        let bank = offset / RAM_BANK_SIZE;
        let bank = if bank < self.ram.len() {
            bank
        } else {
            bank % self.ram.len()
        };
        (bank, offset % RAM_BANK_SIZE)
    }

    pub fn has_camera(&self) -> bool {
        self.mbc.has_camera()
    }

    pub fn set_camera_frame(&mut self, frame: &[u8]) {
        self.mbc.set_camera_frame(frame);
    }

    pub fn has_ir(&self) -> bool {
        self.mbc.has_ir()
    }

    pub fn ir_led(&self) -> bool {
        self.mbc.ir_led()
    }

    pub fn set_ir_light(&mut self, light: bool) {
        self.mbc.set_ir_light(light);
    }

    pub fn has_tilt(&self) -> bool {
        self.mbc.has_tilt()
    }

    pub fn set_tilt(&mut self, x: f32, y: f32) {
        self.mbc.set_tilt(x, y);
    }

    pub fn has_rumble(&self) -> bool {
        self.mbc.has_rumble()
    }

    pub fn take_rumble(&mut self) -> f32 {
        self.mbc.take_rumble()
    }

    pub fn rtc(&self) -> Option<&Rtc> {
        self.mbc.rtc()
    }

    pub fn rtc_mut(&mut self) -> Option<&mut Rtc> {
        self.mbc.rtc_mut()
    }

    #[cfg(feature = "persistence")]
    pub fn poll_battery(
        &mut self,
        force: bool,
        now: u64,
    ) -> Option<crate::persistence::battery_save::BatterySave> {
        if !self.has_battery || (!self.sram_dirty && !force) {
            return None;
        };

        let save = if let Some(data) = self.mbc.get_internal_data(now) {
            crate::persistence::battery_save::BatterySave {
                ram: data,
                clock: None,
                #[cfg(feature = "gba")]
                seiko: None,
                #[cfg(feature = "gba")]
                chip: None,
            }
        } else if let Some(footer) = self.rtc_footer(now) {
            crate::persistence::battery_save::BatterySave {
                ram: self.battery_ram(),
                clock: Some(footer),
                #[cfg(feature = "gba")]
                seiko: None,
                #[cfg(feature = "gba")]
                chip: None,
            }
        } else if let Some(tail) = self.mbc.battery_tail(now) {
            let mut ram = self.battery_ram();
            ram.extend_from_slice(&tail);
            crate::persistence::battery_save::BatterySave {
                ram,
                clock: None,
                #[cfg(feature = "gba")]
                seiko: None,
                #[cfg(feature = "gba")]
                chip: None,
            }
        } else {
            crate::persistence::battery_save::BatterySave {
                ram: self.ram.concat(),
                clock: None,
                #[cfg(feature = "gba")]
                seiko: None,
                #[cfg(feature = "gba")]
                chip: None,
            }
        };

        if !force {
            self.sram_dirty = false;
        }

        Some(save)
    }

    #[cfg(feature = "persistence")]
    pub fn poll_sram_dump(
        &mut self,
        force: bool,
        now: u64,
    ) -> Option<crate::persistence::sram_dump::SramDump> {
        self.poll_battery(force, now)
            .map(|save| crate::persistence::sram_dump::SramDump::from_slice(&save.to_file()))
    }

    #[cfg(feature = "persistence")]
    pub fn read_battery(&self, data: &[u8]) -> crate::persistence::battery_save::BatterySave {
        let ram_len = self.header.ram_banks * RAM_BANK_SIZE;
        if self.mbc.rtc().is_some() && data.len() > ram_len {
            let (ram, footer) = data.split_at(ram_len);
            return crate::persistence::battery_save::BatterySave {
                ram: ram.to_vec(),
                clock: crate::persistence::rtc_footer::RtcFooter::decode(footer),
                #[cfg(feature = "gba")]
                seiko: None,
                #[cfg(feature = "gba")]
                chip: None,
            };
        }
        crate::persistence::battery_save::BatterySave {
            ram: data.to_vec(),
            clock: None,
            #[cfg(feature = "gba")]
            seiko: None,
            #[cfg(feature = "gba")]
            chip: None,
        }
    }

    #[cfg(feature = "persistence")]
    pub fn put_battery(&mut self, save: crate::persistence::battery_save::BatterySave, now: u64) {
        if self.has_battery && !self.mbc.put_internal_data(&save.ram, now) {
            let ram_len = self.header.ram_banks * RAM_BANK_SIZE;
            let (ram, tail) = if self.mbc.battery_tail(now).is_some() {
                save.ram.split_at(ram_len.min(save.ram.len()))
            } else {
                (save.ram.as_slice(), &[][..])
            };
            for (bank, chunk) in self.ram.iter_mut().zip(ram.chunks(RAM_BANK_SIZE)) {
                bank[..chunk.len()].copy_from_slice(chunk);
            }
            self.mbc.put_battery_tail(tail, now);
        }
        if let Some(clock) = save.clock {
            self.put_rtc_footer(clock, now);
        }
    }

    #[cfg(feature = "persistence")]
    pub fn put_sram_dump(&mut self, dump: crate::persistence::sram_dump::SramDump, now: u64) {
        if !self.has_battery {
            return;
        };
        self.put_battery(self.read_battery(dump.as_slice()), now);
    }

    #[cfg(feature = "persistence")]
    pub fn battery_ram(&self) -> Vec<u8> {
        let mut data = self.ram.concat();
        data.truncate(self.header.ram_banks * RAM_BANK_SIZE);
        data
    }

    #[cfg(feature = "persistence")]
    pub fn rtc_footer(&self, now: u64) -> Option<crate::persistence::rtc_footer::RtcFooter> {
        self.mbc
            .rtc()
            .map(|rtc| crate::persistence::rtc_footer::RtcFooter {
                real: rtc.real,
                latched: rtc.real,
                timestamp: now,
            })
    }

    #[cfg(feature = "persistence")]
    pub fn put_rtc_footer(
        &mut self,
        footer: crate::persistence::rtc_footer::RtcFooter,
        now: u64,
    ) -> bool {
        let Some(rtc) = self.mbc.rtc_mut() else {
            return false;
        };
        rtc.real = footer.real;
        rtc.latched = footer.latched;
        rtc.sub_second_cycles = 0;
        rtc.advance_seconds(footer.seconds_until(now));
        true
    }

    pub fn supports_sram_saves(&self) -> bool {
        self.has_battery
    }
}

impl ReadMemory for Cartridge {
    fn read_naive(&self, addr: u16) -> u8 {
        if let Some(value) = self.mbc.on_read(addr) {
            return value;
        };

        match addr {
            0x0000..=0x7FFF => self.rom_byte(self.mbc.rom_offset(addr)),
            0xA000..=0xBFFF => match self.mbc.ram_offset(addr) {
                Some(offset) => {
                    let (bank, index) = self.ram_index(offset);
                    self.ram[bank][index]
                }
                None => 0xFF,
            },
            _ => 0xFF,
        }
    }
}

impl WriteMemory for Cartridge {
    fn write_naive(&mut self, addr: u16, value: u8) {
        let consumed = self.mbc.on_write(addr, value);
        if consumed {
            self.sram_dirty = true;
            return;
        }

        if (0xA000..=0xBFFF).contains(&addr)
            && let Some(offset) = self.mbc.ram_write_offset(addr)
        {
            let (bank, index) = self.ram_index(offset);
            self.ram[bank][index] = value;
            self.sram_dirty = true;
        }
    }
}

#[cfg(feature = "debug")]
impl DisassemblySource for Cartridge {
    fn read_rom_address(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.rom_byte(self.mbc.rom_offset(addr)),
            _ => 0xFF,
        }
    }

    fn rom_address(&self, addr: u16) -> Address {
        match addr {
            0x0000..=0x3FFF => Address::rom(self.mbc.rom_bank_low() as u16, u32::from(addr)),
            _ => Address::rom(
                self.mbc.rom_bank_high() as u16,
                u32::from(addr.wrapping_sub(0x4000) & 0x3FFF),
            ),
        }
    }
}

fn rom_banks_of(rom: &Rom, rom_banks: usize) -> GbResult<Vec<[u8; ROM_BANK_SIZE]>> {
    let banks: Vec<[u8; ROM_BANK_SIZE]> = rom
        .data
        .chunks(ROM_BANK_SIZE)
        .map(|chunk| {
            let mut bank = [0u8; ROM_BANK_SIZE];
            bank[..chunk.len()].copy_from_slice(chunk);
            bank
        })
        .collect();
    if banks.len() > rom_banks {
        return Err(GbError::RomTooBig);
    }
    Ok(banks)
}

fn is_mbc1_multicart(rom: &[[u8; ROM_BANK_SIZE]]) -> bool {
    rom.len() == 64 && rom[0x10][0x104..0x134] == crate::rom::header::NINTENDO_LOGO
}

use crate::cheats::PatchedRom;
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
pub(crate) const NO_ROM_WINDOW: [usize; 4] = [usize::MAX >> 1; 4];

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Cartridge {
    pub header: RomHeader,
    pub has_rom_loaded: bool,
    has_battery: bool,
    sram_dirty: bool,
    mbc: mbc::Mbc,
    #[cfg_attr(feature = "serde", serde(skip, default = "clocked_until_known"))]
    clocked: bool,
    #[cfg_attr(feature = "serde", serde(skip))]
    owes: bool,
    #[cfg_attr(feature = "serde", serde(skip))]
    owed: u32,
    #[cfg_attr(feature = "serde", serde(skip))]
    rom_window: Option<[usize; 2]>,
    #[cfg_attr(feature = "serde", serde(skip, default))]
    rom: Arc<[u8]>,
    #[cfg_attr(feature = "serde", serde(skip, default))]
    ram: Vec<[u8; RAM_BANK_SIZE]>,
    #[cfg_attr(feature = "serde", serde(skip, default))]
    patches: PatchedRom,
}

#[cfg(feature = "serde")]
fn clocked_until_known() -> bool {
    true
}

impl Default for Cartridge {
    fn default() -> Self {
        Self {
            header: RomHeader::default(),
            has_rom_loaded: false,
            has_battery: false,
            sram_dirty: false,
            mbc: mbc::Mbc::None,
            clocked: false,
            owes: false,
            owed: 0,
            rom_window: None,
            rom: vec![0; 2 * ROM_BANK_SIZE].into(),
            ram: vec![[0; RAM_BANK_SIZE]; 1],
            patches: PatchedRom::default(),
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
            clocked: self.clocked,
            owes: self.owes,
            owed: self.owed,
            rom_window: self.rom_window,
            rom: self.rom.clone(),
            ram: self.ram.clone(),
            patches: self.patches.clone(),
        }
    }

    fn clone_from(&mut self, source: &Self) {
        let Self {
            header,
            has_rom_loaded,
            has_battery,
            sram_dirty,
            mbc,
            clocked,
            owes,
            owed,
            rom_window,
            rom,
            ram,
            patches,
        } = self;
        header.clone_from(&source.header);
        *has_rom_loaded = source.has_rom_loaded;
        *has_battery = source.has_battery;
        *sram_dirty = source.sram_dirty;
        mbc.clone_from(&source.mbc);
        *clocked = source.clocked;
        *owes = source.owes;
        *owed = source.owed;
        *rom_window = source.rom_window;
        if !Arc::ptr_eq(rom, &source.rom) {
            *rom = source.rom.clone();
        }
        ram.clone_from(&source.ram);
        patches.clone_from(&source.patches);
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
        self.rom.len() / ROM_BANK_SIZE
    }

    #[cfg(feature = "debug")]
    pub(crate) fn rom_bank(&self, bank: usize) -> Option<&[u8]> {
        let start = bank.checked_mul(ROM_BANK_SIZE)?;
        self.rom.get(start..start + ROM_BANK_SIZE)
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
        self.clocked = self.mbc.is_clocked();
        self.owes = self.clocked && self.mbc.advances_in_bulk();
        self.owed = 0;
        self.rom = like.rom.clone();
        self.patches = like.patches.clone();
        self.ram = vec![[0; RAM_BANK_SIZE]; like.ram.len()];
        for (bank, chunk) in self.ram.iter_mut().zip(ram.chunks(RAM_BANK_SIZE)) {
            bank[..chunk.len()].copy_from_slice(chunk);
        }
        self.map_rom();
    }

    pub fn rom_bytes(&self) -> Arc<[u8]> {
        self.patches.pristine(&self.rom).clone()
    }

    pub(crate) fn pristine_rom(&self) -> &[u8] {
        self.patches.pristine(&self.rom)
    }

    pub(crate) fn set_rom_patches(&mut self, patches: &[(usize, u8)]) {
        self.patches.set(&mut self.rom, patches);
    }

    #[cfg(feature = "persistence")]
    pub fn restore_rom(&mut self, rom: &Rom) -> GbResult<()> {
        let header = rom.header()?;
        let rom_banks = header.rom_banks.max(2);

        self.rom = rom_banks_of(rom, rom_banks)?;
        self.patches = PatchedRom::default();
        self.ram = vec![[0; RAM_BANK_SIZE]; header.ram_banks.max(1)];
        self.clocked = self.mbc.is_clocked();
        self.owes = self.clocked && self.mbc.advances_in_bulk();
        self.owed = 0;
        self.map_rom();
        Ok(())
    }

    pub fn load_rom(&mut self, rom: &Rom) -> GbResult<()> {
        let header = rom.header()?;
        let rom_banks = header.rom_banks.max(2);
        let ram_banks = header.ram_banks.max(1);
        self.mbc = mbc::Mbc::try_from(&header)?;

        let banks = rom_banks_of(rom, rom_banks)?;
        self.mbc.set_multicart(is_mbc1_multicart(&banks));
        self.clocked = self.mbc.is_clocked();
        self.owes = self.clocked && self.mbc.advances_in_bulk();
        self.owed = 0;

        self.has_battery = header
            .cartridge_type
            .map(|ct| ct.has_battery())
            .unwrap_or(false);
        self.header = header;
        self.has_rom_loaded = true;
        self.rom = banks;
        self.patches = PatchedRom::default();
        self.ram = vec![[0; RAM_BANK_SIZE]; ram_banks];
        self.map_rom();

        Ok(())
    }

    pub fn soft_reset(&mut self) {
        self.settle();
        self.mbc.soft_reset();
        self.map_rom();
        if !self.has_battery {
            self.ram.iter_mut().for_each(|bank| bank.fill(0));
        }
    }

    fn map_rom(&mut self) {
        let len = self.rom.len();
        self.rom_window =
            (self.mbc.reads_rom_by_bank() && len > 0 && len.is_multiple_of(ROM_BANK_SIZE))
                .then(|| self.mbc.rom_window_offsets().map(|offset| offset % len));
    }

    pub(crate) fn read_rom(&self, addr: u16) -> u8 {
        match self.rom_window {
            Some(window) => self.read_rom_window(window, addr),
            None => self.read_naive(addr),
        }
    }

    pub(crate) fn rom_window(&self) -> Option<[usize; 4]> {
        self.rom_window.map(|[low, high]| {
            [
                low,
                high.wrapping_sub(ROM_BANK_SIZE),
                NO_ROM_WINDOW[2],
                NO_ROM_WINDOW[3],
            ]
        })
    }

    #[inline(always)]
    pub(crate) fn read_rom_window(&self, window: [usize; 2], addr: u16) -> u8 {
        self.rom[window[usize::from(addr >> 14) & 1] + usize::from(addr & 0x3FFF)]
    }

    #[inline(always)]
    pub(crate) fn rom_window_byte(&self, window: &[usize; 4], addr: u16) -> Option<u8> {
        self.rom
            .get(window[usize::from(addr >> 14)].wrapping_add(usize::from(addr)))
            .copied()
    }

    #[inline(always)]
    pub(crate) fn rom_window_pair(&self, window: &[usize; 4], addr: u16) -> Option<u16> {
        if addr & 0x3FFF == 0x3FFF {
            return None;
        }
        let at = window[usize::from(addr >> 14)].wrapping_add(usize::from(addr));
        let pair = self.rom.get(at..at + 2)?;
        Some(u16::from_le_bytes([pair[0], pair[1]]))
    }

    #[inline]
    pub fn cycle(&mut self) {
        if !self.clocked {
            return;
        }
        if self.owes {
            self.owed += 1;
        } else {
            self.clock_mbc();
        }
    }

    #[inline(never)]
    fn clock_mbc(&mut self) {
        if self.mbc.cycle(&mut self.ram) {
            self.sram_dirty = true;
        }
    }

    pub fn settle(&mut self) {
        if self.owed > 0 {
            self.mbc.advance(std::mem::take(&mut self.owed));
        }
    }

    pub(crate) fn clocked(&self) -> bool {
        self.clocked
    }

    pub fn advances_in_bulk(&self) -> bool {
        !self.clocked || self.mbc.advances_in_bulk()
    }

    pub fn advance(&mut self, cycles: u32) {
        if self.clocked {
            self.owed += cycles;
        }
    }

    fn rom_byte(&self, offset: usize) -> u8 {
        let len = self.rom.len();
        self.rom[if offset < len { offset } else { offset % len }]
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
        self.settle();
        self.mbc.take_rumble()
    }

    pub fn rtc(&self) -> Option<&Rtc> {
        self.mbc.rtc()
    }

    pub fn rtc_mut(&mut self) -> Option<&mut Rtc> {
        self.settle();
        self.mbc.rtc_mut()
    }

    #[cfg(feature = "persistence")]
    pub fn poll_battery(
        &mut self,
        force: bool,
        now: u64,
    ) -> Option<crate::persistence::battery_save::BatterySave> {
        self.settle();
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
        self.settle();
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
        self.map_rom();
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
        self.settle();
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
        self.settle();
        let consumed = self.mbc.on_write(addr, value);
        self.map_rom();
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

fn rom_banks_of(rom: &Rom, rom_banks: usize) -> GbResult<Arc<[u8]>> {
    let size = rom_banks * ROM_BANK_SIZE;
    if rom.data.len() > size {
        return Err(GbError::RomTooBig);
    }
    if rom.data.len() == size {
        return Ok(rom.data.clone());
    }
    let mut banks = rom.data.to_vec();
    banks.resize(size, 0);
    Ok(banks.into())
}

fn is_mbc1_multicart(rom: &[u8]) -> bool {
    const LOGO: usize = 0x10 * ROM_BANK_SIZE;
    rom.len() == 64 * ROM_BANK_SIZE
        && rom[LOGO + 0x104..LOGO + 0x134] == crate::rom::header::NINTENDO_LOGO
}

#[cfg(all(test, feature = "persistence"))]
mod tests {
    use crate::ReadMemory;
    use crate::WriteMemory;
    use crate::console::Console;
    use crate::gb::{GameBoy, GbModel};
    use crate::rom::Rom;

    const NOW: u64 = 1_761_400_000;

    fn rom(cartridge_type: u8, rom_size_code: u8, ram_size_code: u8) -> Vec<u8> {
        let mut data = vec![0u8; 0x8000 << rom_size_code];
        data[0x0147] = cartridge_type;
        data[0x0148] = rom_size_code;
        data[0x0149] = ram_size_code;
        for (bank, chunk) in data.chunks_mut(0x4000).enumerate() {
            chunk[0x2000] = bank as u8;
        }
        data
    }

    fn loaded(data: &[u8]) -> GameBoy {
        let mut gb = GameBoy::new_empty(GbModel::Dmg);
        gb.load_rom(&Rom::new(data)).expect("load");
        gb
    }

    #[test]
    fn mbc30_reaches_rom_banks_past_127_and_eight_ram_banks() {
        let mut gb = loaded(&rom(0x10, 0x07, 0x05));
        gb.cartridge.write_naive(0x2000, 0xC8);
        assert_eq!(gb.cartridge.read_naive(0x6000), 0xC8);
        gb.cartridge.write_naive(0x0000, 0x0A);
        for bank in 0..8 {
            gb.cartridge.write_naive(0x4000, bank);
            gb.cartridge.write_naive(0xA000, 0x40 + bank);
        }
        for bank in 0..8 {
            gb.cartridge.write_naive(0x4000, bank);
            assert_eq!(gb.cartridge.read_naive(0xA000), 0x40 + bank);
        }
    }

    #[test]
    fn rumble_reaches_the_machine() {
        let mut gb = loaded(&rom(0x1C, 0x01, 0x00));
        assert!(gb.has_rumble());
        gb.cartridge.write_naive(0x4000, 0x08);
        (0..100).for_each(|_| gb.cartridge.cycle());
        assert_eq!(gb.take_rumble(), 1.0);
        assert!(!loaded(&rom(0x19, 0x01, 0x00)).has_rumble());
    }

    #[test]
    fn tilt_reaches_the_mbc7_registers() {
        let mut gb = loaded(&rom(0x22, 0x01, 0x00));
        assert!(gb.has_tilt());
        gb.set_tilt(0.0, 1.0);
        gb.cartridge.write_naive(0x0000, 0x0A);
        gb.cartridge.write_naive(0x4000, 0x40);
        gb.cartridge.write_naive(0xA000, 0x55);
        gb.cartridge.write_naive(0xA010, 0xAA);
        let y = u16::from_le_bytes([
            gb.cartridge.read_naive(0xA040),
            gb.cartridge.read_naive(0xA050),
        ]);
        assert_eq!(y, 0x81D0 + 0x70);
    }

    #[test]
    fn mbc7_eeprom_is_the_whole_battery_file() {
        let mut gb = loaded(&rom(0x22, 0x01, 0x00));
        let save = gb.battery(true, NOW).expect("battery");
        assert_eq!(save.to_file(), vec![0xFF; 256]);
        let mut file = vec![0xFF; 256];
        file[0] = 0x12;
        let mut other = loaded(&rom(0x22, 0x01, 0x00));
        let save = other.battery_from_file(&file);
        other.put_battery(save, NOW);
        assert_eq!(other.battery(true, NOW).unwrap().to_file(), file);
    }

    #[test]
    fn huc1_ir_reaches_the_machine() {
        let mut gb = loaded(&rom(0xFF, 0x01, 0x03));
        assert!(gb.cartridge.has_ir());
        gb.cartridge.write_naive(0x0000, 0x0E);
        gb.cartridge.set_ir_light(true);
        assert_eq!(gb.cartridge.read_naive(0xA000), 0xC1);
        gb.cartridge.write_naive(0xA000, 0x01);
        assert!(gb.cartridge.ir_led());
    }

    #[test]
    fn huc3_saves_ram_then_a_17_byte_clock() {
        let mut gb = loaded(&rom(0xFE, 0x01, 0x03));
        gb.cartridge.write_naive(0x0000, 0x0A);
        gb.cartridge.write_naive(0xA000, 0x5A);
        let file = gb.battery(true, NOW).unwrap().to_file();
        assert_eq!(file.len(), 4 * 0x2000 + 17);
        assert_eq!(file[0], 0x5A);
        assert_eq!(&file[4 * 0x2000..4 * 0x2000 + 8], &NOW.to_le_bytes());

        let mut clock = file.clone();
        clock[4 * 0x2000 + 8..4 * 0x2000 + 10].copy_from_slice(&100u16.to_le_bytes());
        let mut other = loaded(&rom(0xFE, 0x01, 0x03));
        let save = other.battery_from_file(&clock);
        other.put_battery(save, NOW + 600);
        let back = other.battery(true, NOW + 600).unwrap().to_file();
        assert_eq!(back[0], 0x5A);
        assert_eq!(
            &back[4 * 0x2000 + 8..4 * 0x2000 + 10],
            &110u16.to_le_bytes()
        );
    }

    #[test]
    fn camera_frames_reach_ram_bank_zero() {
        use crate::gb::cartridge::CAMERA_PIXELS;
        let mut gb = loaded(&rom(0xFC, 0x01, 0x04));
        assert!(gb.has_camera());
        gb.set_camera_frame(&vec![0; CAMERA_PIXELS]);
        gb.cartridge.write_naive(0x4000, 0x10);
        gb.cartridge.write_naive(0xA002, 0x03);
        for cell in 0..16u16 {
            for (i, threshold) in [0x80u8, 0x90, 0xA0].into_iter().enumerate() {
                gb.cartridge
                    .write_naive(0xA006 + cell * 3 + i as u16, threshold);
            }
        }
        gb.cartridge.write_naive(0xA000, 0x03);
        while gb.cartridge.read_naive(0xA000) & 1 != 0 {
            gb.cartridge.cycle();
        }
        gb.cartridge.write_naive(0x4000, 0x00);
        assert_eq!(gb.cartridge.read_naive(0xA100), 0xFF);
        assert_eq!(gb.cartridge.read_naive(0xA100 + 0xDFF), 0xFF);
        assert_eq!(gb.cartridge.read_naive(0xA100 + 0xE00), 0x00);
    }

    fn exchange(gb: &mut GameBoy, byte: u8) -> u8 {
        gb.serial.sb = byte;
        gb.serial.write_sc(0x81, false, &mut gb.ic);
        for _ in 0..16 {
            gb.serial.div_changed(0x80, 0x00, false, &mut gb.ic);
        }
        assert!(!gb.serial.transfer_active());
        gb.serial.sb
    }

    fn packet(command: u8, data: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0x88, 0x33, command, 0x00];
        bytes.extend_from_slice(&(data.len() as u16).to_le_bytes());
        bytes.extend_from_slice(data);
        let sum = bytes[2..]
            .iter()
            .fold(0u16, |sum, &byte| sum.wrapping_add(byte as u16));
        bytes.extend_from_slice(&sum.to_le_bytes());
        bytes.extend_from_slice(&[0x00, 0x00]);
        bytes
    }

    #[test]
    fn a_printer_on_the_serial_port_answers_and_prints() {
        let mut gb = loaded(&rom(0x00, 0x00, 0x00));
        assert_eq!(exchange(&mut gb, 0x88), 0xFF);
        gb.attach_printer(true);
        let replies: Vec<u8> = packet(0x0F, &[])
            .into_iter()
            .map(|byte| exchange(&mut gb, byte))
            .collect();
        assert_eq!(&replies[replies.len() - 2..], &[0x81, 0x00]);
        for byte in packet(0x04, &[0x00; 640]) {
            exchange(&mut gb, byte);
        }
        for byte in packet(0x02, &[1, 0x00, 0xE4, 0x40]) {
            exchange(&mut gb, byte);
        }
        let prints = gb.take_prints();
        assert_eq!(prints.len(), 1);
        assert_eq!((prints[0].width, prints[0].height), (160, 16));
        assert!(prints[0].pixels.iter().all(|&p| p == 255));
        gb.soft_reset();
        assert!(gb.printer_attached());
        gb.attach_printer(false);
        assert_eq!(exchange(&mut gb, 0x88), 0xFF);
    }

    #[test]
    fn cgb_infrared_port_reads_the_light_and_shows_the_led() {
        let mut gb = GameBoy::new_empty(GbModel::Cgb);
        let mut data = rom(0x00, 0x00, 0x00);
        data[0x143] = 0x80;
        gb.load_rom(&Rom::new(&data)).expect("load");
        gb.cgb.cgb_mode = true;
        gb.cgb.write(0xFF56, 0xC0);
        assert_eq!(gb.cgb.read(0xFF56) & 0x02, 0x02);
        gb.set_ir_light(true);
        assert_eq!(gb.cgb.read(0xFF56) & 0x02, 0x00);
        gb.cgb.write(0xFF56, 0x00);
        assert_eq!(gb.cgb.read(0xFF56) & 0x02, 0x02);
        assert!(!gb.ir_led());
        gb.cgb.write(0xFF56, 0x01);
        assert!(gb.ir_led());
    }

    #[test]
    fn tama5_saves_its_ram_and_clock_after_the_cart_ram() {
        let mut gb = loaded(&rom(0xFD, 0x04, 0x00));
        let file = gb.battery(true, NOW).unwrap().to_file();
        assert_eq!(file.len(), 32 + 32 + 8);
        let mut changed = file.clone();
        changed[0] = 0x42;
        let mut other = loaded(&rom(0xFD, 0x04, 0x00));
        let save = other.battery_from_file(&changed);
        other.put_battery(save, NOW);
        assert_eq!(other.battery(true, NOW).unwrap().to_file()[0], 0x42);
    }

    #[test]
    fn mbc6_saves_ram_then_the_flash() {
        let mut gb = loaded(&rom(0x20, 0x06, 0x03));
        let file = gb.battery(true, NOW).unwrap().to_file();
        assert_eq!(file.len(), 4 * 0x2000 + 0x10_0000);
        let mut changed = file.clone();
        changed[4 * 0x2000 + 5] = 0x17;
        let mut other = loaded(&rom(0x20, 0x06, 0x03));
        let save = other.battery_from_file(&changed);
        other.put_battery(save, NOW);
        other.cartridge.write_naive(0x0C00, 0x01);
        other.cartridge.write_naive(0x2800, 0x08);
        other.cartridge.write_naive(0x2000, 0x00);
        assert_eq!(other.cartridge.read_naive(0x4005), 0x17);
    }

    #[test]
    fn mmm01_boots_the_menu_then_maps_the_game() {
        let mut data = rom(0x01, 0x05, 0x00);
        let menu = data.len() - 0x8000;
        data[menu + 0x104..menu + 0x134].copy_from_slice(&crate::rom::header::NINTENDO_LOGO);
        data[menu + 0x147] = 0x0B;
        let mut gb = loaded(&data);
        assert_eq!(gb.cartridge.read_naive(0x2000), 62);
        assert_eq!(gb.cartridge.read_naive(0x6000), 63);
        gb.cartridge.write_naive(0x2000, 0x20);
        gb.cartridge.write_naive(0x6000, 0x1C << 1);
        gb.cartridge.write_naive(0x0000, 0x40);
        assert_eq!(gb.cartridge.read_naive(0x2000), 0x20);
        assert_eq!(gb.cartridge.read_naive(0x6000), 0x21);
    }

    #[test]
    fn wisdom_tree_switches_32k_by_address() {
        let mut data = rom(0x00, 0x00, 0x00);
        data.resize(0x20000, 0);
        for (bank, chunk) in data.chunks_mut(0x4000).enumerate() {
            chunk[0x2000] = bank as u8;
        }
        data[0x134..0x13F].copy_from_slice(b"WISDOM TREE");
        let mut gb = loaded(&data);
        gb.cartridge.write_naive(0x0002, 0x00);
        assert_eq!(gb.cartridge.read_naive(0x2000), 4);
        assert_eq!(gb.cartridge.read_naive(0x6000), 5);
    }

    fn marked(mut data: Vec<u8>) -> Vec<u8> {
        for (bank, chunk) in data.chunks_mut(0x4000).enumerate() {
            chunk[0x1000] = bank as u8 ^ 0x5A;
            chunk[0x3FFF] = bank as u8 ^ 0xA5;
        }
        data
    }

    fn assert_rom_reads_match(gb: &GameBoy, kind: u8) {
        for addr in [
            0x0000, 0x1000, 0x2000, 0x3FFF, 0x4000, 0x5000, 0x6000, 0x7FFF,
        ] {
            assert_eq!(
                gb.cartridge.read_rom(addr),
                gb.cartridge.read_naive(addr),
                "{kind:02X} at {addr:04X}"
            );
        }
    }

    #[test]
    fn rom_reads_through_the_bank_window_match_the_mapper() {
        let mut mmm01 = rom(0x01, 0x05, 0x00);
        let menu = mmm01.len() - 0x8000;
        mmm01[menu + 0x104..menu + 0x134].copy_from_slice(&crate::rom::header::NINTENDO_LOGO);
        mmm01[menu + 0x147] = 0x0B;
        let mut wisdom_tree = rom(0x00, 0x00, 0x00);
        wisdom_tree.resize(0x20000, 0);
        wisdom_tree[0x134..0x13F].copy_from_slice(b"WISDOM TREE");
        let mut carts = [
            (0x00, 0x00, 0x00),
            (0x01, 0x06, 0x00),
            (0x03, 0x05, 0x03),
            (0x05, 0x03, 0x00),
            (0x10, 0x06, 0x03),
            (0x13, 0x07, 0x05),
            (0x19, 0x06, 0x00),
            (0x1C, 0x01, 0x00),
            (0x20, 0x06, 0x03),
            (0x22, 0x01, 0x00),
            (0xFC, 0x01, 0x04),
            (0xFD, 0x04, 0x00),
            (0xFE, 0x01, 0x03),
            (0xFF, 0x01, 0x03),
        ]
        .map(|(kind, rom_size, ram_size)| (kind, marked(rom(kind, rom_size, ram_size))))
        .to_vec();
        carts.push((0x0B, marked(mmm01)));
        carts.push((0xEE, marked(wisdom_tree)));

        let mut seed = 0x2545_F491_u32;
        for (kind, data) in &carts {
            let mut gb = loaded(data);
            assert_rom_reads_match(&gb, *kind);
            for _ in 0..3000 {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let addr = (seed >> 16) as u16 & 0x7FFF;
                let addr = if seed & 0x10 != 0 {
                    addr
                } else {
                    0xA000 | (addr & 0x1FFF)
                };
                gb.cartridge.write_naive(addr, (seed >> 8) as u8);
                assert_rom_reads_match(&gb, *kind);
            }
            let mut copy = loaded(data);
            copy.cartridge.clone_from(&gb.cartridge);
            assert_rom_reads_match(&copy, *kind);
            gb.cartridge.soft_reset();
            assert_rom_reads_match(&gb, *kind);
        }
    }
}

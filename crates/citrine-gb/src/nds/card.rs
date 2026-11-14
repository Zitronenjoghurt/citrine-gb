mod key1;
mod key2;

use std::sync::Arc;

use super::aux_spi::AuxDevice;
use super::ir::IrPort;
use super::save::Backup;
use crate::traffic::{Ended, Entry, Kind, Traffic};
use key1::Key1;
pub use key1::{KEY_TABLE_BYTES, KEY_TABLE_OFFSET, KeyTable};
use key2::Key2;

pub const AUX_CONTROL: usize = 0x00;
pub const ROM_CONTROL: usize = 0x04;
pub const COMMAND_LOW: usize = 0x08;
pub const COMMAND_HIGH: usize = 0x0C;
pub const SEED0_LOW: usize = 0x10;
pub const SEED1_LOW: usize = 0x14;
pub const SEED_HIGH: usize = 0x18;

const AUX_BAUD: u16 = 0x0003;
const AUX_HOLD: u16 = 1 << 6;
const AUX_BUSY: u16 = 1 << 7;
const AUX_SERIAL: u16 = 1 << 13;
const AUX_IRQ: u16 = 1 << 14;
const AUX_ENABLE: u16 = 1 << 15;
const AUX_WRITABLE: u16 = AUX_BAUD | AUX_HOLD | AUX_SERIAL | AUX_IRQ | AUX_ENABLE;
const AUX_DATA_SHIFT: u32 = 16;
const SPI_CYCLES_PER_BYTE_AT_4MHZ: u32 = 64;

const GAP1: u32 = 0x1FFF;
const KEY2_DATA: u32 = 1 << 13;
const APPLY_SEED: u32 = 1 << 15;
const GAP2_SHIFT: u32 = 16;
const GAP2: u32 = 0x3F << GAP2_SHIFT;
const KEY2_COMMAND: u32 = 1 << 22;
const DATA_READY: u32 = 1 << 23;
const BLOCK_SHIFT: u32 = 24;
const BLOCK_SIZE: u32 = 7 << BLOCK_SHIFT;
const SLOW_CLOCK: u32 = 1 << 27;
const GAP_CLOCKS: u32 = 1 << 28;
const RELEASE_RESET: u32 = 1 << 29;
const BUSY: u32 = 1 << 31;
const ROM_WRITABLE: u32 = 0x7F7F_7FFF;

const SEED_HIGH_MASK: u64 = 0x7F;
const BLOCK_GAP_BYTES: u32 = 0x200;
const FAST_BYTE_CYCLES: u32 = 5;
const SLOW_BYTE_CYCLES: u32 = 8;
const COMMAND_BYTES: u32 = 8;
const WORD_BYTES: u32 = 4;

const SECURE_AREA_START: usize = 0x4000;
const SECURE_AREA_END: usize = 0x8000;
const ENCRYPTED_BYTES: usize = 0x800;
const SECURE_AREA_ID: &[u8; 8] = b"encryObj";
const BOOTED_ID: u64 = 0xE7FF_DEFF_E7FF_DEFF;
const GAMECODE: usize = 0x0C;
const SECURE_AREA_CRC: usize = 0x6C;
const SEED_SELECT: usize = 0x13;
const ARM9_ROM_OFFSET: usize = 0x20;
const HEADER_REPEAT: usize = 0x1000;
const DATA_REDIRECT: u32 = 0x8000;
const DATA_BLOCK: u32 = 0xFFF;
const MASKROM_MAKER: u32 = 0xC2;
const RESET_SEED0: u64 = 0x58_C56D_E0E8;
const HIGH_Z: u8 = 0xFF;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
enum Mode {
    #[default]
    Raw,
    Key1,
    Main,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
enum Reply {
    #[default]
    HighZ,
    Header,
    ChipId,
    Zeros,
    SecureArea(u32),
    Data(u32),
}

#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Card {
    #[cfg_attr(feature = "serde", serde(skip))]
    rom: Arc<[u8]>,
    key1: Option<Key1>,
    key2: Key2,
    pending_seed: Option<u64>,
    mode: Mode,
    reply: Reply,
    offset: u32,
}

impl Card {
    fn new(rom: Arc<[u8]>, table: Option<&KeyTable>) -> Self {
        let mut card = Self {
            key1: table.map(|table| Key1::for_commands(table, gamecode(&rom))),
            ..Self::default()
        };
        card.insert(rom, table);
        card
    }

    fn insert(&mut self, rom: Arc<[u8]>, table: Option<&KeyTable>) {
        let Some(table) = table.filter(|_| has_secure_area(&rom)) else {
            self.rom = rom;
            return;
        };
        let area = &rom[SECURE_AREA_START..SECURE_AREA_START + ENCRYPTED_BYTES];
        if read_u64(area) != BOOTED_ID && &area[..8] != SECURE_AREA_ID {
            self.rom = rom;
            return;
        }
        let mut image = rom.to_vec();
        let gamecode = gamecode(&image);
        let area = &mut image[SECURE_AREA_START..SECURE_AREA_START + ENCRYPTED_BYTES];
        area[..8].copy_from_slice(SECURE_AREA_ID);
        encrypt_secure_area(area, table, gamecode);
        self.rom = image.into();
    }

    fn chip_id(&self) -> u32 {
        let megabytes = (self.rom.len().next_power_of_two() >> 20).max(1) as u32;
        MASKROM_MAKER | (megabytes - 1) << 8
    }

    fn command(&mut self, mut bytes: [u8; 8]) {
        if let Some(seed0) = self.pending_seed.take() {
            self.key2 = Key2::new(seed0, key2::SEED1);
        }
        self.offset = 0;
        match self.mode {
            Mode::Raw => self.raw_command(bytes),
            Mode::Key1 => match &self.key1 {
                Some(key) => {
                    let plain = key.decrypt(u64::from_be_bytes(bytes));
                    self.key1_command(plain);
                }
                None => self.reply = Reply::Zeros,
            },
            Mode::Main => {
                self.key2.apply(&mut bytes);
                self.main_command(bytes);
            }
        }
    }

    fn raw_command(&mut self, bytes: [u8; 8]) {
        self.reply = match bytes[0] {
            0x00 => Reply::Header,
            0x90 => Reply::ChipId,
            0x3C => {
                self.mode = Mode::Key1;
                self.key2 = Key2::new(RESET_SEED0, key2::SEED1);
                Reply::HighZ
            }
            _ => Reply::HighZ,
        };
    }

    fn key1_command(&mut self, command: u64) {
        self.reply = match command >> 60 {
            0x1 => Reply::ChipId,
            0x2 => Reply::SecureArea(((command >> 44) & 0xFFFF) as u32 * 0x1000),
            0x4 => {
                let parameter = ((command >> 20) & 0xFF_FFFF) as u32;
                let select = self.rom.get(SEED_SELECT).copied().unwrap_or(0);
                self.pending_seed = Some(key2::seed0(parameter, select));
                Reply::Zeros
            }
            0xA => {
                self.mode = Mode::Main;
                Reply::Zeros
            }
            _ => Reply::Zeros,
        };
    }

    fn main_command(&mut self, bytes: [u8; 8]) {
        self.reply = match bytes[0] {
            0xB7 => {
                let address = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
                let address = if address < DATA_REDIRECT {
                    DATA_REDIRECT + (address & 0x1FF)
                } else {
                    address
                };
                Reply::Data(address)
            }
            0xB8 => Reply::ChipId,
            _ => Reply::Zeros,
        };
    }

    fn shares_key2(&self, key2: &Key2) -> bool {
        self.mode != Mode::Raw && self.key2 == *key2
    }

    fn plain_word(&mut self) -> u32 {
        let word = self.reply_word();
        self.key2.skip(WORD_BYTES);
        word
    }

    fn next_word(&mut self) -> u32 {
        let word = self.reply_word();
        if self.mode == Mode::Raw {
            word
        } else {
            word ^ self.key2.next_word()
        }
    }

    fn reply_word(&mut self) -> u32 {
        if let Reply::Data(address) = self.reply {
            let start = address.wrapping_add(self.offset) & DATA_BLOCK;
            let at = ((address & !DATA_BLOCK) | start) as usize;
            if start <= DATA_BLOCK - 3
                && let Some(bytes) = self.rom.get(at..).and_then(|rest| rest.first_chunk())
            {
                self.offset = self.offset.wrapping_add(WORD_BYTES);
                return u32::from_le_bytes(*bytes);
            }
        }
        u32::from_le_bytes([0; 4].map(|_| self.reply_byte()))
    }

    fn reply_byte(&mut self) -> u8 {
        let offset = self.offset;
        self.offset = self.offset.wrapping_add(1);
        match self.reply {
            Reply::HighZ => HIGH_Z,
            Reply::Zeros => 0,
            Reply::Header => self.rom_byte(offset as usize % HEADER_REPEAT),
            Reply::ChipId => self.chip_id().to_le_bytes()[offset as usize % 4],
            Reply::SecureArea(base) => self.rom_byte((base + (offset & DATA_BLOCK)) as usize),
            Reply::Data(address) => {
                let address = (address & !DATA_BLOCK) | (address.wrapping_add(offset) & DATA_BLOCK);
                self.rom_byte(address as usize)
            }
        }
    }

    fn dummy(&mut self) {
        if self.mode != Mode::Raw {
            self.key2.next();
        }
    }

    fn rom_byte(&self, address: usize) -> u8 {
        self.rom.get(address).copied().unwrap_or(HIGH_Z)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Transfer {
    words_left: u32,
    bytes_sent: u32,
    cycles_left: u32,
    word: u32,
    ready: bool,
}

#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Cartridge {
    card: Card,
    aux: AuxDevice,
    clock: u64,
    aux_control: u16,
    aux_data: u8,
    aux_incoming: u8,
    aux_busy_cycles: u32,
    aux_selected: bool,
    rom_control: u32,
    command: [u8; 8],
    seed0: u64,
    seed1: u64,
    key2: Key2,
    transfer: Transfer,
    interrupt: bool,
    #[cfg_attr(feature = "serde", serde(skip))]
    armed: Armed,
    #[cfg_attr(feature = "serde", serde(skip))]
    aux_traffic: Traffic,
    #[cfg_attr(feature = "serde", serde(skip))]
    card_traffic: Traffic,
}

#[derive(Debug, Clone, Copy, Default)]
struct Armed {
    rom: bool,
    aux: bool,
    since: u32,
}

impl Cartridge {
    pub fn new(rom: &[u8], key_table: Option<&KeyTable>, aux: AuxDevice) -> Self {
        Self::with_rom(rom.into(), key_table, aux)
    }

    pub fn with_rom(rom: Arc<[u8]>, key_table: Option<&KeyTable>, aux: AuxDevice) -> Self {
        Self {
            card: Card::new(rom, key_table),
            aux,
            ..Self::default()
        }
    }

    pub fn reinsert(&mut self, rom: Arc<[u8]>, key_table: Option<&KeyTable>) {
        self.card.insert(rom, key_table);
    }

    pub fn direct_boot(&mut self) {
        let select = self.card.rom.get(SEED_SELECT).copied().unwrap_or(0);
        let seed0 = key2::seed0(0, select);
        self.card.mode = Mode::Main;
        self.card.pending_seed = None;
        self.card.key2 = Key2::new(seed0, key2::SEED1);
        self.seed0 = seed0;
        self.seed1 = key2::SEED1;
        self.key2 = Key2::new(seed0, key2::SEED1);
    }

    pub fn chip_id(&self) -> u32 {
        self.card.chip_id()
    }

    pub fn aux(&self) -> &AuxDevice {
        &self.aux
    }

    pub fn aux_mut(&mut self) -> &mut AuxDevice {
        &mut self.aux
    }

    pub fn backup(&self) -> Option<&Backup> {
        self.aux.backup()
    }

    pub fn adopt_rom(&mut self, from: &Cartridge) {
        self.card.rom = Arc::clone(&from.card.rom);
    }

    pub fn adopt_aux(&mut self, old: &mut Cartridge) {
        old.deselect_backup_for(Ended::Reset);
        self.aux = std::mem::take(&mut old.aux);
        self.clock = old.clock;
        self.aux_traffic.adopt(&mut old.aux_traffic);
        self.card_traffic.adopt(&mut old.card_traffic);
        self.aux_traffic
            .push(Entry::new(Kind::SoftReset).at(self.clock));
        self.card_traffic
            .push(Entry::new(Kind::SoftReset).at(self.clock));
    }

    pub fn exchange_ir(&mut self, port: &mut dyn IrPort) {
        if !self.aux.has_infrared() {
            return;
        }
        let mut logged = LoggedIr {
            port,
            traffic: &mut self.aux_traffic,
        };
        self.aux.exchange(&mut logged, self.clock);
    }

    pub fn clock(&self) -> u64 {
        self.clock
    }

    pub fn aux_traffic(&self) -> &Traffic {
        &self.aux_traffic
    }

    pub fn card_traffic(&self) -> &Traffic {
        &self.card_traffic
    }

    pub fn next_traffic_frame(&mut self) {
        self.aux_traffic.next_frame();
        self.card_traffic.next_frame();
    }

    pub fn read(&self, offset: usize) -> u32 {
        match offset & !3 {
            AUX_CONTROL => {
                let busy = if self.aux_busy_cycles > 0 {
                    AUX_BUSY
                } else {
                    0
                };
                u32::from(self.aux_control | busy) | u32::from(self.aux_data) << AUX_DATA_SHIFT
            }
            ROM_CONTROL => {
                let ready = if self.transfer.ready { DATA_READY } else { 0 };
                let busy = if self.busy() { BUSY } else { 0 };
                self.rom_control | ready | busy
            }
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: usize, value: u32, mask: u32) {
        match offset & !3 {
            AUX_CONTROL => {
                if mask & 0xFFFF != 0 {
                    self.write_aux_control(merge16(self.aux_control, value, mask));
                }
                if mask >> AUX_DATA_SHIFT & 0xFF != 0 {
                    self.write_aux_data((value >> AUX_DATA_SHIFT) as u8);
                }
            }
            ROM_CONTROL => self.write_rom_control(value, mask),
            COMMAND_LOW => self.write_command(0, value, mask),
            COMMAND_HIGH => self.write_command(4, value, mask),
            SEED0_LOW => self.seed0 = merge_low(self.seed0, value, mask),
            SEED1_LOW => self.seed1 = merge_low(self.seed1, value, mask),
            SEED_HIGH => {
                self.seed0 = merge_high(self.seed0, value, mask & 0xFFFF);
                self.seed1 = merge_high(self.seed1, value >> 16, mask >> 16);
            }
            _ => {}
        }
    }

    pub fn peek_data(&self) -> u32 {
        self.transfer.word
    }

    pub fn read_data(&mut self) -> u32 {
        if !self.transfer.ready {
            return self.transfer.word;
        }
        self.transfer.ready = false;
        self.transfer.words_left -= 1;
        if self.transfer.words_left == 0 {
            self.finish();
        } else {
            self.schedule_word();
        }
        self.transfer.word
    }

    pub fn delay(&mut self, cycles: u32) {
        if self.transfer.cycles_left > 0 {
            self.transfer.cycles_left += cycles;
        }
    }

    pub fn dma_request(&self) -> bool {
        self.transfer.ready
    }

    pub fn until_event(&self) -> Option<u32> {
        [self.transfer.cycles_left, self.aux_busy_cycles]
            .into_iter()
            .filter(|&cycles| cycles > 0)
            .min()
    }

    pub fn armed(&self) -> bool {
        self.armed.rom || self.armed.aux
    }

    pub fn step(&mut self, cycles: u32) {
        self.clock += u64::from(cycles);
        self.count_aux(cycles);
        self.count_transfer(cycles);
    }

    pub fn step_slice(&mut self, cycles: u32) {
        self.clock += u64::from(cycles);
        if !self.armed.aux {
            self.count_aux(cycles);
        }
        if !self.armed.rom {
            self.count_transfer(cycles);
        }
    }

    pub fn armed_since(&mut self, cycles: u32) {
        self.armed.since = cycles;
    }

    pub fn settle(&mut self) {
        let armed = std::mem::take(&mut self.armed);
        if armed.aux {
            self.count_aux(armed.since);
        }
        if armed.rom {
            self.count_transfer(armed.since);
        }
    }

    fn count_aux(&mut self, cycles: u32) {
        if self.aux_busy_cycles > 0 {
            self.aux_busy_cycles = self.aux_busy_cycles.saturating_sub(cycles);
            if self.aux_busy_cycles == 0 {
                self.aux_data = self.aux_incoming;
            }
        }
    }

    fn count_transfer(&mut self, cycles: u32) {
        if self.transfer.cycles_left == 0 {
            return;
        }
        self.transfer.cycles_left = self.transfer.cycles_left.saturating_sub(cycles);
        if self.transfer.cycles_left > 0 {
            return;
        }
        if self.transfer.words_left == 0 {
            self.finish();
            return;
        }
        self.transfer.word = self.data_word();
        self.transfer.ready = true;
    }

    pub fn take_interrupt(&mut self) -> bool {
        std::mem::take(&mut self.interrupt)
    }

    fn write_aux_control(&mut self, value: u16) {
        let old = self.aux_control;
        self.aux_control = value & AUX_WRITABLE;
        if (old ^ self.aux_control) & !AUX_HOLD != 0 {
            self.aux_traffic.push(
                Entry::new(Kind::AuxControl)
                    .at(self.clock)
                    .sent(&self.aux_control.to_le_bytes()),
            );
        }
        let serial = AUX_ENABLE | AUX_SERIAL;
        if self.aux_selected && self.aux_control & serial != serial {
            self.deselect_backup_for(Ended::ControlWrite);
        }
    }

    fn write_aux_data(&mut self, value: u8) {
        let serial = AUX_ENABLE | AUX_SERIAL;
        if self.aux_control & serial != serial || self.aux_busy_cycles > 0 {
            return;
        }
        self.aux_selected = true;
        self.aux_incoming = self.aux.transfer(value, self.clock);
        self.aux_traffic.byte(
            Kind::AuxSpi,
            (self.aux_control & AUX_BAUD) as u8,
            self.clock,
            value,
            self.aux_incoming,
        );
        if self.aux_control & AUX_HOLD == 0 {
            self.deselect_backup_for(Ended::HoldOff);
        }
        self.aux_busy_cycles = SPI_CYCLES_PER_BYTE_AT_4MHZ << (self.aux_control & AUX_BAUD);
        self.armed.aux = true;
    }

    fn deselect_backup_for(&mut self, ended: Ended) {
        self.aux.deselect(self.clock);
        self.aux_selected = false;
        self.aux_traffic.close(ended);
    }

    fn write_rom_control(&mut self, value: u32, mask: u32) {
        let sticky = self.rom_control & RELEASE_RESET;
        self.rom_control = (self.rom_control & !mask) | (value & mask & ROM_WRITABLE) | sticky;
        if value & mask & APPLY_SEED != 0 {
            self.key2 = Key2::new(self.seed0, self.seed1);
        }
        if value & mask & BUSY != 0 && !self.busy() && self.aux_control & AUX_ENABLE != 0 {
            self.start();
        }
    }

    fn write_command(&mut self, first: usize, value: u32, mask: u32) {
        for lane in 0..4 {
            if mask >> (lane * 8) & 0xFF != 0 {
                self.command[first + lane] = (value >> (lane * 8)) as u8;
            }
        }
    }

    fn busy(&self) -> bool {
        self.transfer.words_left > 0 || self.transfer.cycles_left > 0
    }

    fn start(&mut self) {
        let mut command = self.command;
        if self.rom_control & KEY2_COMMAND != 0 {
            self.key2.apply(&mut command);
        }
        self.card.command(command);
        self.card_traffic.push(
            Entry::new(Kind::CardCommand)
                .at(self.clock)
                .sent(&self.command)
                .got(&self.block_bytes().to_le_bytes()),
        );
        let gap = self.gap1() + self.gap2();
        self.dummy_bytes(gap);
        let words = self.block_bytes() / WORD_BYTES;
        let lead = COMMAND_BYTES + gap + if words > 0 { WORD_BYTES } else { 0 };
        self.transfer = Transfer {
            words_left: words,
            bytes_sent: 0,
            cycles_left: lead * self.byte_cycles(),
            word: self.transfer.word,
            ready: false,
        };
        self.armed.rom = true;
    }

    fn schedule_word(&mut self) {
        let mut bytes = WORD_BYTES;
        if self.transfer.bytes_sent.is_multiple_of(BLOCK_GAP_BYTES) {
            let gap = self.gap2();
            self.dummy_bytes(gap);
            bytes += gap;
        }
        self.transfer.cycles_left = bytes * self.byte_cycles();
        self.armed.rom = true;
    }

    fn finish(&mut self) {
        self.transfer = Transfer {
            word: self.transfer.word,
            ..Transfer::default()
        };
        if self.aux_control & AUX_IRQ != 0 {
            self.interrupt = true;
        }
    }

    fn data_word(&mut self) -> u32 {
        self.transfer.bytes_sent += WORD_BYTES;
        let scrambled = self.rom_control & KEY2_DATA != 0;
        if scrambled && self.card.shares_key2(&self.key2) {
            let word = self.card.plain_word();
            self.key2 = self.card.key2;
            return word;
        }
        let word = self.card.next_word();
        if scrambled {
            word ^ self.key2.next_word()
        } else {
            word
        }
    }

    fn dummy_bytes(&mut self, count: u32) {
        if self.rom_control & GAP_CLOCKS == 0 {
            return;
        }
        for _ in 0..count {
            self.card.dummy();
            if self.rom_control & KEY2_DATA != 0 {
                self.key2.next();
            }
        }
    }

    fn block_bytes(&self) -> u32 {
        match (self.rom_control & BLOCK_SIZE) >> BLOCK_SHIFT {
            0 => 0,
            7 => 4,
            size => 0x100 << size,
        }
    }

    fn byte_cycles(&self) -> u32 {
        if self.rom_control & SLOW_CLOCK != 0 {
            SLOW_BYTE_CYCLES
        } else {
            FAST_BYTE_CYCLES
        }
    }

    fn gap1(&self) -> u32 {
        self.rom_control & GAP1
    }

    fn gap2(&self) -> u32 {
        (self.rom_control & GAP2) >> GAP2_SHIFT
    }
}

struct LoggedIr<'a> {
    port: &'a mut dyn IrPort,
    traffic: &'a mut Traffic,
}

impl IrPort for LoggedIr<'_> {
    fn send(&mut self, packet: &[u8], at: u64) {
        self.traffic
            .push(Entry::new(Kind::IrSent).at(at).sent(packet));
        self.port.send(packet, at);
    }

    fn take(&mut self, now: u64) -> Option<Vec<u8>> {
        let packet = self.port.take(now)?;
        self.traffic
            .push(Entry::new(Kind::IrReceived).at(now).sent(&packet));
        Some(packet)
    }
}

pub fn encrypt_secure_area(area: &mut [u8], table: &KeyTable, gamecode: u32) {
    let level3 = Key1::new(table, gamecode, 3);
    for block in area[..ENCRYPTED_BYTES].as_chunks_mut::<8>().0 {
        *block = level3.encrypt(u64::from_le_bytes(*block)).to_le_bytes();
    }
    let level2 = Key1::new(table, gamecode, 2);
    write_u64(area, level2.encrypt(read_u64(area)));
}

pub fn decrypt_secure_area(area: &mut [u8], table: &KeyTable, gamecode: u32) {
    let level2 = Key1::new(table, gamecode, 2);
    write_u64(area, level2.decrypt(read_u64(area)));
    let level3 = Key1::new(table, gamecode, 3);
    for block in area[..ENCRYPTED_BYTES].as_chunks_mut::<8>().0 {
        *block = level3.decrypt(u64::from_le_bytes(*block)).to_le_bytes();
    }
}

pub fn secure_area_encrypted(rom: &[u8]) -> bool {
    if !has_secure_area(rom) {
        return false;
    }
    let area = &rom[SECURE_AREA_START..SECURE_AREA_END];
    let first = read_u64(area);
    let stored = u16::from_le_bytes([rom[SECURE_AREA_CRC], rom[SECURE_AREA_CRC + 1]]);
    first != 0
        && first != BOOTED_ID
        && &area[..8] != SECURE_AREA_ID
        && crate::utils::crc16::crc16(0xFFFF, area) == stored
}

fn has_secure_area(rom: &[u8]) -> bool {
    let Some(offset) = rom.get(ARM9_ROM_OFFSET..ARM9_ROM_OFFSET + 4) else {
        return false;
    };
    let offset = u32::from_le_bytes([offset[0], offset[1], offset[2], offset[3]]) as usize;
    (SECURE_AREA_START..SECURE_AREA_END).contains(&offset) && rom.len() >= SECURE_AREA_END
}

fn gamecode(rom: &[u8]) -> u32 {
    rom.get(GAMECODE..GAMECODE + 4).map_or(0, |code| {
        u32::from_le_bytes([code[0], code[1], code[2], code[3]])
    })
}

fn read_u64(bytes: &[u8]) -> u64 {
    let mut word = [0; 8];
    word.copy_from_slice(&bytes[..8]);
    u64::from_le_bytes(word)
}

fn write_u64(bytes: &mut [u8], value: u64) {
    bytes[..8].copy_from_slice(&value.to_le_bytes());
}

fn merge16(old: u16, value: u32, mask: u32) -> u16 {
    ((u32::from(old) & !mask) | (value & mask)) as u16
}

fn merge_low(seed: u64, value: u32, mask: u32) -> u64 {
    let low = (seed as u32 & !mask) | (value & mask);
    (seed & !0xFFFF_FFFF) | u64::from(low)
}

fn merge_high(seed: u64, value: u32, mask: u32) -> u64 {
    if mask & 0xFF == 0 {
        return seed;
    }
    (seed & 0xFFFF_FFFF) | (u64::from(value) & SEED_HIGH_MASK) << 32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nds::save::SaveType;

    const GAMECODE_TEXT: &[u8; 4] = b"CTRN";
    const KEY1_CONTROL: u32 = 0x0018_08F8;
    const DATA_CONTROL: u32 = 0x0058_6000;
    const PARAMETER: u64 = 0x12_3456;
    const SECURE_RANDOM: u64 = 0x98_7654 << 20;
    const RANDOM: u64 = 0xABCD << 44 | SECURE_RANDOM;
    const HEADER: u32 = 1;
    const CHIP_ID: u32 = 7;
    const SECURE_BLOCK: u32 = 4;

    fn key_table() -> KeyTable {
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let bytes: Vec<u8> = (0..KEY_TABLE_BYTES)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 24) as u8
            })
            .collect();
        KeyTable::from_bytes(&bytes).unwrap()
    }

    fn homebrew_rom() -> Vec<u8> {
        let mut rom: Vec<u8> = (0..0x2_0000u32)
            .map(|index| (index * 7 + index / 251) as u8)
            .collect();
        rom[..0x0C].copy_from_slice(b"CITRINE TEST");
        rom[GAMECODE..GAMECODE + 4].copy_from_slice(GAMECODE_TEXT);
        rom[SEED_SELECT] = 2;
        rom[ARM9_ROM_OFFSET..ARM9_ROM_OFFSET + 4].copy_from_slice(&0x4000u32.to_le_bytes());
        rom[SECURE_AREA_START..SECURE_AREA_START + 8].copy_from_slice(SECURE_AREA_ID);
        rom
    }

    fn gamecode_value() -> u32 {
        u32::from_le_bytes(*GAMECODE_TEXT)
    }

    struct Host {
        cart: Cartridge,
        counter: u64,
    }

    impl Host {
        fn new(cart: Cartridge) -> Self {
            let mut host = Self { cart, counter: 0 };
            host.cart.write(AUX_CONTROL, u32::from(AUX_ENABLE), 0xFFFF);
            host
        }

        fn seed(&mut self, seed0: u64, seed1: u64) {
            self.cart.write(SEED0_LOW, seed0 as u32, u32::MAX);
            self.cart.write(SEED1_LOW, seed1 as u32, u32::MAX);
            let high = (seed0 >> 32) as u32 | ((seed1 >> 32) as u32) << 16;
            self.cart.write(SEED_HIGH, high, u32::MAX);
            self.cart.write(ROM_CONTROL, APPLY_SEED, APPLY_SEED);
        }

        fn start(&mut self, command: [u8; 8], control: u32, size: u32) {
            let low = u32::from_le_bytes([command[0], command[1], command[2], command[3]]);
            let high = u32::from_le_bytes([command[4], command[5], command[6], command[7]]);
            self.cart.write(COMMAND_LOW, low, u32::MAX);
            self.cart.write(COMMAND_HIGH, high, u32::MAX);
            self.cart
                .write(ROM_CONTROL, control | size << BLOCK_SHIFT | BUSY, u32::MAX);
        }

        fn send(&mut self, command: [u8; 8], control: u32, size: u32) -> Vec<u8> {
            self.start(command, control, size);
            let mut bytes = Vec::new();
            while self.cart.read(ROM_CONTROL) & BUSY != 0 {
                self.cart.step(1);
                if self.cart.dma_request() {
                    bytes.extend(self.cart.read_data().to_le_bytes());
                }
            }
            bytes
        }

        fn send_key1(&mut self, command: u64, control: u32, size: u32) -> Vec<u8> {
            let counter = self.counter;
            self.counter += 1;
            let key = Key1::for_commands(&key_table(), gamecode_value());
            let encrypted = key.encrypt(command | counter);
            self.send(encrypted.to_be_bytes(), control, size)
        }

        fn enter_key1(&mut self) {
            self.send([0x3C, 0, 0, 0, 0, 0, 0, 0], 0, 0);
            self.seed(RESET_SEED0, key2::SEED1);
        }

        fn enter_key2(&mut self) {
            self.send_key1(0x4 << 60 | 0xABCD << 44 | PARAMETER << 20, KEY1_CONTROL, 0);
            self.seed(key2::seed0(PARAMETER as u32, 2), key2::SEED1);
        }

        fn enter_main(&mut self) {
            self.enter_key1();
            self.enter_key2();
            self.send_key1(0xA << 60 | RANDOM, KEY1_CONTROL | KEY2_DATA, 0);
        }

        fn read_main(&mut self, address: u32) -> Vec<u8> {
            let mut command = [0xB7, 0, 0, 0, 0, 0, 0, 0];
            command[1..5].copy_from_slice(&address.to_be_bytes());
            self.send(command, DATA_CONTROL, HEADER)
        }
    }

    fn cartridge() -> Cartridge {
        Cartridge::new(&homebrew_rom(), Some(&key_table()), AuxDevice::Empty)
    }

    fn host() -> Host {
        Host::new(cartridge())
    }

    #[test]
    fn a_homebrew_rom_boots_through_every_command_stage() {
        let rom = homebrew_rom();
        let mut host = host();

        let dummy = host.send([0x9F, 0, 0, 0, 0, 0, 0, 0], 0, 5);
        assert_eq!(dummy.len(), 0x2000);
        assert!(dummy.iter().all(|&byte| byte == HIGH_Z));
        assert_eq!(host.send([0; 8], 0, HEADER), rom[..0x200]);
        assert_eq!(
            host.send([0x90, 0, 0, 0, 0, 0, 0, 0], 0, CHIP_ID),
            [0xC2, 0, 0, 0]
        );

        host.enter_key1();
        host.enter_key2();
        let chip_id = host.send_key1(0x1 << 60 | RANDOM, KEY1_CONTROL | KEY2_DATA, CHIP_ID);
        assert_eq!(chip_id, [0xC2, 0, 0, 0]);

        let mut secure = Vec::new();
        for block in [5u64, 7, 4, 6] {
            let bytes = host.send_key1(
                0x2 << 60 | block << 44 | SECURE_RANDOM,
                KEY1_CONTROL | KEY2_DATA,
                SECURE_BLOCK,
            );
            secure.push((block, bytes));
        }
        secure.sort();
        let mut secure: Vec<u8> = secure.into_iter().flat_map(|(_, bytes)| bytes).collect();
        assert_ne!(secure[..8], *SECURE_AREA_ID);
        decrypt_secure_area(&mut secure, &key_table(), gamecode_value());
        assert_eq!(secure, rom[SECURE_AREA_START..SECURE_AREA_END]);

        host.send_key1(0xA << 60 | RANDOM, KEY1_CONTROL | KEY2_DATA, 0);
        assert_eq!(host.read_main(0x8000), rom[0x8000..0x8200]);
        assert_eq!(host.read_main(0x1_2000), rom[0x1_2000..0x1_2200]);
        let chip_id = host.send([0xB8, 0, 0, 0, 0, 0, 0, 0], DATA_CONTROL, CHIP_ID);
        assert_eq!(chip_id, [0xC2, 0, 0, 0]);
    }

    #[test]
    fn the_header_repeats_every_4k() {
        let mut host = host();
        let header = host.send([0; 8], 0, 5);
        assert_eq!(header[..0x1000], header[0x1000..]);
        assert_eq!(header[..0x1000], homebrew_rom()[..0x1000]);
    }

    #[test]
    fn data_reads_below_8000_redirect_into_the_first_block() {
        let rom = homebrew_rom();
        let mut host = host();
        host.enter_main();
        assert_eq!(host.read_main(0x1234), rom[0x8034..0x8234]);
    }

    #[test]
    fn data_reads_wrap_inside_their_4k_block() {
        let rom = homebrew_rom();
        let mut host = host();
        host.enter_main();
        let bytes = host.read_main(0x9F00);
        assert_eq!(bytes[..0x100], rom[0x9F00..0xA000]);
        assert_eq!(bytes[0x100..], rom[0x9000..0x9100]);
    }

    #[test]
    fn key2_replies_are_scrambled_when_the_controller_leaves_them() {
        let mut host = host();
        host.enter_key1();
        host.enter_key2();
        let raw = host.send_key1(0x1 << 60 | RANDOM, KEY1_CONTROL, CHIP_ID);
        assert_ne!(raw, [0xC2, 0, 0, 0]);
    }

    #[test]
    fn a_wrong_seed_garbles_the_data() {
        let rom = homebrew_rom();
        let mut host = host();
        host.enter_main();
        host.seed(0x11_2233_4455, key2::SEED1);
        assert_ne!(host.read_main(0x8000), rom[0x8000..0x8200]);
    }

    #[test]
    fn dummy_gap_clocks_keep_both_key2_streams_in_step() {
        let mut host = host();
        host.enter_key1();
        host.enter_key2();
        let control = KEY1_CONTROL | GAP_CLOCKS | KEY2_DATA;
        assert_eq!(
            host.send_key1(0x1 << 60 | RANDOM, control, CHIP_ID),
            [0xC2, 0, 0, 0]
        );
        assert_eq!(
            host.send_key1(0x1 << 60 | RANDOM, KEY1_CONTROL | KEY2_DATA, CHIP_ID),
            [0xC2, 0, 0, 0]
        );
    }

    #[test]
    fn a_read_with_both_key2_streams_in_step_keeps_them_in_step() {
        let rom = homebrew_rom();
        let mut host = host();
        host.enter_main();
        let before = host.cart.key2;
        assert_eq!(
            host.read_main(0x8FFE),
            rom[0x8FFE..0x9000]
                .iter()
                .chain(&rom[0x8000..0x81FE])
                .copied()
                .collect::<Vec<_>>()
        );
        assert_ne!(host.cart.key2, before);
        assert_eq!(host.cart.key2, host.cart.card.key2);
    }

    #[test]
    fn a_word_read_with_streams_apart_matches_reading_each_byte() {
        let rom = homebrew_rom();
        let mut host = host();
        host.enter_main();
        host.cart.key2.next();
        let mut card = host.cart.card.key2;
        let mut controller = host.cart.key2;
        let mut command = [0xB7, 0, 0, 0, 0, 0, 0, 0];
        command[1..5].copy_from_slice(&0x8000u32.to_be_bytes());
        card.apply(&mut command);
        let expected: Vec<u8> = rom[0x8000..0x8200]
            .iter()
            .map(|&byte| byte ^ card.next() ^ controller.next())
            .collect();
        let control = DATA_CONTROL & !KEY2_COMMAND;
        assert_eq!(host.send(command, control, HEADER), expected);
        assert_ne!(host.cart.key2, host.cart.card.key2);
    }

    #[test]
    fn direct_boot_starts_in_main_data_mode() {
        let rom = homebrew_rom();
        let mut host = host();
        host.cart.direct_boot();
        assert_eq!(host.read_main(0x8200), rom[0x8200..0x8400]);
    }

    #[test]
    fn a_booted_secure_area_is_encrypted_back_for_the_bios() {
        let mut rom = homebrew_rom();
        rom[SECURE_AREA_START..SECURE_AREA_START + 8].copy_from_slice(&BOOTED_ID.to_le_bytes());
        let mut host = Host::new(Cartridge::new(&rom, Some(&key_table()), AuxDevice::Empty));
        host.enter_key1();
        host.enter_key2();
        let mut block = host.send_key1(
            0x2 << 60 | 4 << 44 | SECURE_RANDOM,
            KEY1_CONTROL | KEY2_DATA,
            SECURE_BLOCK,
        );
        decrypt_secure_area(&mut block, &key_table(), gamecode_value());
        assert_eq!(&block[..8], SECURE_AREA_ID);
        assert_eq!(
            block[8..],
            rom[SECURE_AREA_START + 8..SECURE_AREA_START + 0x1000]
        );
    }

    #[test]
    fn without_a_key_table_the_secure_area_stays_as_stored() {
        let rom = homebrew_rom();
        let card = Card::new(rom.clone().into(), None);
        assert_eq!(card.rom[..], rom[..]);
    }

    #[test]
    fn without_a_key_table_key1_commands_read_zeros() {
        let mut host = Host::new(Cartridge::new(&homebrew_rom(), None, AuxDevice::Empty));
        host.send([0x3C, 0, 0, 0, 0, 0, 0, 0], 0, 0);
        host.seed(RESET_SEED0, key2::SEED1);
        assert_eq!(
            host.send([0x15; 8], KEY1_CONTROL | KEY2_DATA, CHIP_ID),
            [0; 4]
        );
    }

    #[test]
    fn secure_area_encryption_round_trips() {
        let original = homebrew_rom()[SECURE_AREA_START..SECURE_AREA_END].to_vec();
        let mut area = original.clone();
        encrypt_secure_area(&mut area, &key_table(), gamecode_value());
        assert_ne!(area[..ENCRYPTED_BYTES], original[..ENCRYPTED_BYTES]);
        assert_eq!(area[ENCRYPTED_BYTES..], original[ENCRYPTED_BYTES..]);
        decrypt_secure_area(&mut area, &key_table(), gamecode_value());
        assert_eq!(area, original);
    }

    #[test]
    fn chip_id_carries_the_size_in_megabytes() {
        let mut rom = homebrew_rom();
        rom.resize(0x80_0000, 0);
        let mut host = Host::new(Cartridge::new(&rom, None, AuxDevice::Empty));
        assert_eq!(
            host.send([0x90, 0, 0, 0, 0, 0, 0, 0], 0, CHIP_ID),
            [0xC2, 7, 0, 0]
        );
    }

    #[test]
    fn the_first_word_waits_for_command_gap_and_word_bytes() {
        let mut host = host();
        let control = 0x10 | 0x02 << GAP2_SHIFT;
        host.start([0; 8], control, HEADER);
        let lead = (8 + 0x10 + 0x02 + 4) * FAST_BYTE_CYCLES;
        host.cart.step(lead - 1);
        assert_eq!(host.cart.read(ROM_CONTROL) & DATA_READY, 0);
        host.cart.step(1);
        assert_ne!(host.cart.read(ROM_CONTROL) & DATA_READY, 0);
        assert!(host.cart.dma_request());
    }

    #[test]
    fn the_slow_clock_takes_eight_cycles_a_byte() {
        let mut host = host();
        host.start([0; 8], SLOW_CLOCK, HEADER);
        host.cart.step(12 * SLOW_BYTE_CYCLES - 1);
        assert!(!host.cart.dma_request());
        host.cart.step(1);
        assert!(host.cart.dma_request());
    }

    #[test]
    fn an_unread_word_holds_the_transfer() {
        let mut host = host();
        host.start([0; 8], 0, HEADER);
        host.cart.step(1_000);
        let first = host.cart.read_data();
        assert_eq!(first.to_le_bytes(), homebrew_rom()[..4]);
        assert!(!host.cart.dma_request());
        assert_eq!(host.cart.read_data(), first);
        host.cart.step(4 * FAST_BYTE_CYCLES);
        assert_eq!(host.cart.read_data().to_le_bytes(), homebrew_rom()[4..8]);
    }

    #[test]
    fn busy_clears_and_the_irq_fires_after_the_last_word() {
        let mut host = host();
        host.cart
            .write(AUX_CONTROL, u32::from(AUX_ENABLE | AUX_IRQ), 0xFFFF);
        host.start([0x90, 0, 0, 0, 0, 0, 0, 0], 0, CHIP_ID);
        host.cart.step(1_000);
        assert_ne!(host.cart.read(ROM_CONTROL) & BUSY, 0);
        assert!(!host.cart.take_interrupt());
        host.cart.read_data();
        assert_eq!(host.cart.read(ROM_CONTROL) & (BUSY | DATA_READY), 0);
        assert!(host.cart.take_interrupt());
        assert!(!host.cart.take_interrupt());
    }

    #[test]
    fn a_command_without_data_ends_after_its_bytes() {
        let mut host = host();
        host.cart
            .write(AUX_CONTROL, u32::from(AUX_ENABLE | AUX_IRQ), 0xFFFF);
        host.start([0x3C, 0, 0, 0, 0, 0, 0, 0], 0, 0);
        host.cart.step(8 * FAST_BYTE_CYCLES - 1);
        assert_ne!(host.cart.read(ROM_CONTROL) & BUSY, 0);
        host.cart.step(1);
        assert_eq!(host.cart.read(ROM_CONTROL) & BUSY, 0);
        assert!(host.cart.take_interrupt());
    }

    #[test]
    fn the_irq_stays_off_without_its_enable_bit() {
        let mut host = host();
        host.send([0x90, 0, 0, 0, 0, 0, 0, 0], 0, CHIP_ID);
        assert!(!host.cart.take_interrupt());
    }

    #[test]
    fn a_disabled_slot_starts_no_transfer() {
        let mut host = host();
        host.cart.write(AUX_CONTROL, 0, 0xFFFF);
        host.start([0; 8], 0, HEADER);
        assert_eq!(host.cart.read(ROM_CONTROL) & BUSY, 0);
    }

    #[test]
    fn reset_release_cannot_be_cleared() {
        let mut host = host();
        host.cart.write(ROM_CONTROL, RELEASE_RESET, u32::MAX);
        host.cart.write(ROM_CONTROL, 0, u32::MAX);
        assert_ne!(host.cart.read(ROM_CONTROL) & RELEASE_RESET, 0);
        assert_eq!(host.cart.read(ROM_CONTROL) & APPLY_SEED, 0);
    }

    #[test]
    fn four_byte_blocks_end_after_one_word() {
        let mut host = host();
        assert_eq!(host.send([0; 8], 0, CHIP_ID).len(), 4);
    }

    #[test]
    fn auxspi_reaches_the_save_chip() {
        let backup = Backup::new(SaveType::Eeprom64K);
        let mut cart = Cartridge::new(&homebrew_rom(), None, AuxDevice::Save(backup));
        let serial = u32::from(AUX_ENABLE | AUX_SERIAL);
        let send = |cart: &mut Cartridge, byte: u8, hold: bool| {
            let hold = if hold { u32::from(AUX_HOLD) } else { 0 };
            cart.write(AUX_CONTROL, serial | hold, 0xFFFF);
            cart.write(
                AUX_CONTROL,
                u32::from(byte) << AUX_DATA_SHIFT,
                0xFF << AUX_DATA_SHIFT,
            );
            assert_ne!(cart.read(AUX_CONTROL) as u16 & AUX_BUSY, 0);
            cart.step(SPI_CYCLES_PER_BYTE_AT_4MHZ);
            (cart.read(AUX_CONTROL) >> AUX_DATA_SHIFT) as u8
        };
        send(&mut cart, 0x06, false);
        send(&mut cart, 0x05, true);
        assert_eq!(send(&mut cart, 0, false), 0x02);
    }

    #[test]
    fn auxspi_is_silent_in_rom_mode() {
        let mut cart = Cartridge::new(
            &homebrew_rom(),
            None,
            AuxDevice::Save(Backup::new(SaveType::Eeprom64K)),
        );
        cart.write(AUX_CONTROL, u32::from(AUX_ENABLE), 0xFFFF);
        cart.write(AUX_CONTROL, 0x06 << AUX_DATA_SHIFT, 0xFF << AUX_DATA_SHIFT);
        assert_eq!(cart.read(AUX_CONTROL) as u16 & AUX_BUSY, 0);
    }

    fn with_secure_crc(mut rom: Vec<u8>) -> Vec<u8> {
        let crc = crate::utils::crc16::crc16(0xFFFF, &rom[SECURE_AREA_START..SECURE_AREA_END]);
        rom[SECURE_AREA_CRC..SECURE_AREA_CRC + 2].copy_from_slice(&crc.to_le_bytes());
        rom
    }

    #[test]
    fn an_encrypted_secure_area_is_told_by_its_checksum() {
        let mut rom = homebrew_rom();
        encrypt_secure_area(
            &mut rom[SECURE_AREA_START..SECURE_AREA_END],
            &key_table(),
            gamecode_value(),
        );
        let mut rom = with_secure_crc(rom);
        assert!(secure_area_encrypted(&rom));
        rom[SECURE_AREA_CRC] ^= 1;
        assert!(!secure_area_encrypted(&rom));
    }

    #[test]
    fn plain_and_homebrew_secure_areas_are_not_encrypted() {
        let mut booted = homebrew_rom();
        booted[SECURE_AREA_START..SECURE_AREA_START + 8].copy_from_slice(&BOOTED_ID.to_le_bytes());
        let mut zeroed = homebrew_rom();
        zeroed[SECURE_AREA_START..SECURE_AREA_START + 8].fill(0);
        for rom in [homebrew_rom(), booted, zeroed] {
            assert!(!secure_area_encrypted(&with_secure_crc(rom)));
        }
    }
}

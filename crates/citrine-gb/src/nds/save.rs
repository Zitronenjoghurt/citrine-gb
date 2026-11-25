use std::sync::Arc;

const ERASED: u8 = 0xFF;
const IDLE_OUTPUT: u8 = 0xFF;

const WRITE_STATUS: u8 = 0x01;
const WRITE: u8 = 0x02;
const READ: u8 = 0x03;
const WRITE_DISABLE: u8 = 0x04;
const READ_STATUS: u8 = 0x05;
const WRITE_ENABLE: u8 = 0x06;
const PAGE_WRITE: u8 = 0x0A;
const FAST_READ: u8 = 0x0B;
const READ_ID: u8 = 0x9F;
const WAKE: u8 = 0xAB;
const SLEEP: u8 = 0xB9;
const CHIP_ERASE: u8 = 0xC7;
const SECTOR_ERASE: u8 = 0xD8;
const PAGE_ERASE: u8 = 0xDB;

const UPPER_HALF: u8 = 0x08;
const STATUS_WRITE_ENABLED: u8 = 0x02;
const STATUS_PROTECT: u8 = 0x0C;
const STATUS_LOCK: u8 = 0x80;
const STATUS_TINY_ONES: u8 = 0xF0;
const FLASH_PAGE: usize = 0x100;
const FLASH_SECTOR: usize = 0x1_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SaveType {
    Eeprom512,
    Eeprom8K,
    Eeprom64K,
    Eeprom128K,
    Flash256K,
    Flash512K,
    Flash1M,
    Flash8M,
    Fram8K,
    Fram32K,
}

impl SaveType {
    pub fn size(self) -> usize {
        match self {
            Self::Eeprom512 => 0x200,
            Self::Eeprom8K | Self::Fram8K => 0x2000,
            Self::Eeprom64K => 0x1_0000,
            Self::Eeprom128K => 0x2_0000,
            Self::Fram32K => 0x8000,
            Self::Flash256K => 0x4_0000,
            Self::Flash512K => 0x8_0000,
            Self::Flash1M => 0x10_0000,
            Self::Flash8M => 0x80_0000,
        }
    }

    pub fn is_flash(self) -> bool {
        matches!(
            self,
            Self::Flash256K | Self::Flash512K | Self::Flash1M | Self::Flash8M
        )
    }

    pub fn for_size(len: usize) -> Option<Self> {
        [
            Self::Eeprom512,
            Self::Eeprom8K,
            Self::Fram32K,
            Self::Eeprom64K,
            Self::Eeprom128K,
            Self::Flash256K,
            Self::Flash512K,
            Self::Flash1M,
            Self::Flash8M,
        ]
        .into_iter()
        .find(|kind| len <= kind.size())
        .filter(|_| len > 0)
    }

    fn address_bytes(self) -> u8 {
        match self {
            Self::Eeprom512 => 1,
            Self::Eeprom8K | Self::Eeprom64K | Self::Fram8K | Self::Fram32K => 2,
            _ => 3,
        }
    }

    fn page_size(self) -> usize {
        match self {
            Self::Eeprom512 => 0x10,
            Self::Eeprom8K => 0x20,
            Self::Eeprom64K => 0x80,
            Self::Fram8K | Self::Fram32K => self.size(),
            _ => FLASH_PAGE,
        }
    }

    fn id(self) -> [u8; 3] {
        match self {
            Self::Flash256K => [0x20, 0x40, 0x12],
            Self::Flash512K => [0x20, 0x40, 0x13],
            Self::Flash1M => [0x20, 0x40, 0x14],
            Self::Flash8M => [0xC2, 0x20, 0x17],
            _ => [IDLE_OUTPUT; 3],
        }
    }

    fn status_writable(self) -> u8 {
        match self {
            Self::Eeprom512 => STATUS_PROTECT,
            _ if self.is_flash() => 0,
            _ => STATUS_PROTECT | STATUS_LOCK,
        }
    }

    fn status_fixed(self) -> u8 {
        if self == Self::Eeprom512 {
            STATUS_TINY_ONES
        } else {
            0
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
enum Command {
    Waiting,
    Ignored,
    ReadStatus,
    WriteStatus,
    ReadId(usize),
    Read,
    Write,
    Program,
    EraseRange(usize),
}

impl Command {
    fn needs_write_enable(self) -> bool {
        matches!(
            self,
            Self::WriteStatus | Self::Write | Self::Program | Self::EraseRange(_)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Backup {
    kind: SaveType,
    #[cfg_attr(feature = "serde", serde(with = "crate::utils::bytes::shared"))]
    data: Arc<[u8]>,
    status: u8,
    write_enabled: bool,
    asleep: bool,
    command: Command,
    uses_latch: bool,
    address: usize,
    address_left: u8,
    dummy_left: u8,
    #[cfg_attr(
        feature = "serde",
        serde(serialize_with = "crate::utils::unsaved::serialize")
    )]
    dirty: bool,
}

impl Backup {
    pub fn new(kind: SaveType) -> Self {
        Self {
            kind,
            data: vec![ERASED; kind.size()].into(),
            status: 0,
            write_enabled: false,
            asleep: false,
            command: Command::Waiting,
            uses_latch: false,
            address: 0,
            address_left: 0,
            dummy_left: 0,
            dirty: false,
        }
    }

    pub fn kind(&self) -> SaveType {
        self.kind
    }

    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    pub fn load(&mut self, bytes: &[u8]) {
        let data = Arc::make_mut(&mut self.data);
        let len = bytes.len().min(data.len());
        data[..len].copy_from_slice(&bytes[..len]);
        data[len..].fill(ERASED);
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    pub fn transfer(&mut self, byte: u8) -> u8 {
        if self.command == Command::Waiting {
            self.begin(byte);
            return IDLE_OUTPUT;
        }
        if self.address_left > 0 {
            self.address = (self.address << 8) | usize::from(byte);
            self.address_left -= 1;
            if self.address_left == 0 {
                self.address %= self.data.len();
            }
            return IDLE_OUTPUT;
        }
        if self.dummy_left > 0 {
            self.dummy_left -= 1;
            return IDLE_OUTPUT;
        }
        self.data_byte(byte)
    }

    pub fn deselect(&mut self) {
        if let Command::EraseRange(size) = self.command
            && self.address_left == 0
        {
            self.erase(size);
        }
        if self.uses_latch {
            self.write_enabled = false;
        }
        self.uses_latch = false;
        self.command = Command::Waiting;
        self.address_left = 0;
        self.dummy_left = 0;
    }

    fn begin(&mut self, byte: u8) {
        self.address = 0;
        self.command = self.decode(byte);
        if self.command.needs_write_enable() && !self.write_enabled {
            self.command = Command::Ignored;
        }
        self.uses_latch = self.command.needs_write_enable();
    }

    fn decode(&mut self, byte: u8) -> Command {
        let flash = self.kind.is_flash();
        if self.asleep {
            if byte == WAKE {
                self.asleep = false;
            }
            return Command::Ignored;
        }
        match byte {
            WRITE_ENABLE => {
                self.write_enabled = true;
                Command::Ignored
            }
            WRITE_DISABLE => {
                self.write_enabled = false;
                Command::Ignored
            }
            READ_STATUS => Command::ReadStatus,
            READ_ID => Command::ReadId(0),
            WRITE_STATUS if !flash => Command::WriteStatus,
            SLEEP if flash => {
                self.asleep = true;
                Command::Ignored
            }
            READ => self.addressed_command(Command::Read, 0),
            FAST_READ if flash => {
                self.dummy_left = 1;
                self.addressed_command(Command::Read, 0)
            }
            WRITE if flash => self.addressed_command(Command::Program, 0),
            PAGE_WRITE if flash => self.addressed_command(Command::Write, 0),
            PAGE_ERASE if flash => self.addressed_command(Command::EraseRange(FLASH_PAGE), 0),
            SECTOR_ERASE if flash => self.addressed_command(Command::EraseRange(FLASH_SECTOR), 0),
            CHIP_ERASE if flash => Command::EraseRange(self.data.len()),
            WRITE | PAGE_WRITE | FAST_READ if self.kind == SaveType::Eeprom512 => {
                let half = usize::from(byte & UPPER_HALF != 0);
                let command = if byte & 1 != 0 {
                    Command::Read
                } else {
                    Command::Write
                };
                self.addressed_command(command, half)
            }
            WRITE => self.addressed_command(Command::Write, 0),
            _ => Command::Ignored,
        }
    }

    fn addressed_command(&mut self, command: Command, high: usize) -> Command {
        self.address = high;
        self.address_left = self.kind.address_bytes();
        command
    }

    fn data_byte(&mut self, byte: u8) -> u8 {
        match self.command {
            Command::ReadStatus => self.status(),
            Command::WriteStatus => {
                let writable = self.kind.status_writable();
                self.status = (self.status & !writable) | (byte & writable);
                self.command = Command::Ignored;
                IDLE_OUTPUT
            }
            Command::ReadId(index) => {
                self.command = Command::ReadId(index + 1);
                self.kind.id().get(index).copied().unwrap_or(IDLE_OUTPUT)
            }
            Command::Read => {
                let value = self.data[self.address];
                self.address = (self.address + 1) % self.data.len();
                value
            }
            Command::Write => {
                self.store(byte);
                IDLE_OUTPUT
            }
            Command::Program => {
                let programmed = self.data[self.address] & byte;
                self.store(programmed);
                IDLE_OUTPUT
            }
            Command::Waiting | Command::Ignored | Command::EraseRange(_) => IDLE_OUTPUT,
        }
    }

    fn status(&self) -> u8 {
        let enabled = if self.write_enabled {
            STATUS_WRITE_ENABLED
        } else {
            0
        };
        self.kind.status_fixed() | self.status | enabled
    }

    fn store(&mut self, value: u8) {
        if !self.protected(self.address) && self.data[self.address] != value {
            Arc::make_mut(&mut self.data)[self.address] = value;
            self.dirty = true;
        }
        let page = self.kind.page_size();
        self.address = (self.address & !(page - 1)) | ((self.address + 1) & (page - 1));
    }

    fn protected(&self, address: usize) -> bool {
        let quarters = match (self.status & STATUS_PROTECT) >> 2 {
            0 => 0,
            1 => 1,
            2 => 2,
            _ => 4,
        };
        address >= self.data.len() - self.data.len() / 4 * quarters
    }

    fn erase(&mut self, size: usize) {
        let start = self.address & !(size - 1);
        let range = start..start + size;
        if self.data[range.clone()].iter().any(|&byte| byte != ERASED) {
            Arc::make_mut(&mut self.data)[range].fill(ERASED);
            self.dirty = true;
        }
    }
}

const SAVE_TYPES: &[u8] = include_bytes!("../../assets/nds-saves/saves.bin");
const SAVE_TYPE_ENTRY: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Listing {
    Chip(SaveType),
    NoSave,
    Unlisted,
}

pub fn listing(game_code: [u8; 4]) -> Listing {
    let entries = SAVE_TYPES.as_chunks::<SAVE_TYPE_ENTRY>().0;
    let key = u32::from_le_bytes(game_code);
    let found = entries.binary_search_by_key(&key, |row| {
        u32::from_le_bytes([row[0], row[1], row[2], row[3]])
    });
    match found {
        Ok(at) => chip(entries[at][4]).map_or(Listing::NoSave, Listing::Chip),
        Err(_) => Listing::Unlisted,
    }
}

fn chip(code: u8) -> Option<SaveType> {
    Some(match code {
        1 => SaveType::Eeprom512,
        2 => SaveType::Eeprom8K,
        3 => SaveType::Eeprom64K,
        4 => SaveType::Eeprom128K,
        5 => SaveType::Flash256K,
        6 => SaveType::Flash512K,
        7 => SaveType::Flash1M,
        8 => SaveType::Flash8M,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(chip: &mut Backup, bytes: &[u8]) -> Vec<u8> {
        let out = bytes.iter().map(|&byte| chip.transfer(byte)).collect();
        chip.deselect();
        out
    }

    fn address(kind: SaveType, address: usize) -> Vec<u8> {
        let bytes = address.to_be_bytes();
        bytes[bytes.len() - usize::from(kind.address_bytes())..].to_vec()
    }

    fn write(chip: &mut Backup, opcode: u8, at: usize, data: &[u8]) {
        command(chip, &[WRITE_ENABLE]);
        let mut bytes = vec![opcode];
        bytes.extend(address(chip.kind(), at));
        bytes.extend_from_slice(data);
        command(chip, &bytes);
    }

    fn read(chip: &mut Backup, opcode: u8, at: usize, len: usize) -> Vec<u8> {
        let mut bytes = vec![opcode];
        bytes.extend(address(chip.kind(), at));
        let head = bytes.len();
        bytes.extend(std::iter::repeat_n(0, len));
        command(chip, &bytes)[head..].to_vec()
    }

    fn status(chip: &mut Backup) -> u8 {
        command(chip, &[READ_STATUS, 0])[1]
    }

    fn with_bytes(kind: SaveType) -> Backup {
        let mut chip = Backup::new(kind);
        let bytes: Vec<u8> = (0..kind.size()).map(|i| (i % 251) as u8).collect();
        chip.load(&bytes);
        chip
    }

    #[test]
    fn every_chip_starts_erased_at_its_size() {
        for kind in [
            SaveType::Eeprom512,
            SaveType::Eeprom8K,
            SaveType::Eeprom64K,
            SaveType::Eeprom128K,
            SaveType::Flash256K,
            SaveType::Flash512K,
            SaveType::Flash1M,
            SaveType::Flash8M,
            SaveType::Fram8K,
            SaveType::Fram32K,
        ] {
            let chip = Backup::new(kind);
            assert_eq!(chip.bytes().len(), kind.size(), "{kind:?}");
            assert!(chip.bytes().iter().all(|&byte| byte == ERASED), "{kind:?}");
        }
        assert_eq!(SaveType::Flash512K.size(), 512 * 1024);
    }

    #[test]
    fn raw_sav_bytes_round_trip() {
        let chip = with_bytes(SaveType::Flash512K);
        let mut copy = Backup::new(SaveType::Flash512K);
        copy.load(chip.bytes());
        assert_eq!(copy.bytes(), chip.bytes());
        assert!(!copy.dirty());
    }

    #[test]
    fn a_short_sav_fills_the_rest_erased() {
        let mut chip = with_bytes(SaveType::Eeprom8K);
        chip.load(&[1, 2, 3]);
        assert_eq!(&chip.bytes()[..4], [1, 2, 3, ERASED]);
        assert!(chip.bytes()[3..].iter().all(|&byte| byte == ERASED));
    }

    #[test]
    fn a_long_sav_keeps_the_chip_size() {
        let mut chip = Backup::new(SaveType::Eeprom512);
        chip.load(&[7; 0x400]);
        assert_eq!(chip.bytes(), [7; 0x200]);
    }

    #[test]
    fn write_enable_sets_the_latch_and_write_disable_clears_it() {
        let mut chip = Backup::new(SaveType::Eeprom64K);
        command(&mut chip, &[WRITE_ENABLE]);
        assert_eq!(status(&mut chip), STATUS_WRITE_ENABLED);
        command(&mut chip, &[WRITE_DISABLE]);
        assert_eq!(status(&mut chip), 0);
    }

    #[test]
    fn read_status_repeats_while_clocked() {
        let mut chip = Backup::new(SaveType::Fram32K);
        command(&mut chip, &[WRITE_ENABLE]);
        assert_eq!(command(&mut chip, &[READ_STATUS, 0, 0, 0])[1..], [2, 2, 2]);
    }

    #[test]
    fn eeprom_512_reads_the_lower_half() {
        let mut chip = with_bytes(SaveType::Eeprom512);
        assert_eq!(read(&mut chip, READ, 0x10, 3), [0x10, 0x11, 0x12]);
    }

    #[test]
    fn eeprom_512_reads_the_upper_half_with_0b() {
        let mut chip = with_bytes(SaveType::Eeprom512);
        assert_eq!(
            read(&mut chip, FAST_READ, 0x10, 2),
            [(0x110 % 251) as u8, (0x111 % 251) as u8]
        );
    }

    #[test]
    fn eeprom_512_writes_both_halves() {
        let mut chip = Backup::new(SaveType::Eeprom512);
        write(&mut chip, WRITE, 0x20, &[0xAA]);
        write(&mut chip, PAGE_WRITE, 0x20, &[0xBB]);
        assert_eq!(chip.bytes()[0x20], 0xAA);
        assert_eq!(chip.bytes()[0x120], 0xBB);
        assert!(chip.dirty());
    }

    #[test]
    fn eeprom_512_reads_roll_over_the_whole_chip() {
        let mut chip = with_bytes(SaveType::Eeprom512);
        assert_eq!(
            read(&mut chip, FAST_READ, 0xFF, 2),
            [(0x1FF % 251) as u8, 0]
        );
    }

    #[test]
    fn eeprom_512_page_writes_wrap_inside_16_bytes() {
        let mut chip = Backup::new(SaveType::Eeprom512);
        write(&mut chip, WRITE, 0x1E, &[1, 2, 3]);
        assert_eq!(chip.bytes()[0x10], 3);
        assert_eq!(chip.bytes()[0x20], ERASED);
    }

    #[test]
    fn eeprom_512_status_has_the_upper_bits_set() {
        let mut chip = Backup::new(SaveType::Eeprom512);
        assert_eq!(status(&mut chip), 0xF0);
        command(&mut chip, &[WRITE_ENABLE]);
        command(&mut chip, &[WRITE_STATUS, 0xFF]);
        assert_eq!(status(&mut chip), 0xFC);
    }

    #[test]
    fn writes_without_write_enable_are_ignored() {
        let mut chip = Backup::new(SaveType::Eeprom8K);
        command(&mut chip, &[WRITE, 0x00, 0x10, 0x55]);
        assert_eq!(chip.bytes()[0x10], ERASED);
        assert!(!chip.dirty());
    }

    #[test]
    fn a_write_clears_the_write_enable_latch() {
        let mut chip = Backup::new(SaveType::Eeprom8K);
        write(&mut chip, WRITE, 0x10, &[0x55]);
        assert_eq!(status(&mut chip), 0);
        command(&mut chip, &[WRITE, 0x00, 0x11, 0x66]);
        assert_eq!(chip.bytes()[0x11], ERASED);
    }

    #[test]
    fn eeprom_8k_uses_two_address_bytes() {
        let mut chip = Backup::new(SaveType::Eeprom8K);
        write(&mut chip, WRITE, 0x1234, &[0x42, 0x43]);
        assert_eq!(chip.bytes()[0x1234..0x1236], [0x42, 0x43]);
        assert_eq!(read(&mut chip, READ, 0x1234, 2), [0x42, 0x43]);
    }

    #[test]
    fn eeprom_8k_page_writes_wrap_inside_32_bytes() {
        let mut chip = Backup::new(SaveType::Eeprom8K);
        write(&mut chip, WRITE, 0x3F, &[1, 2]);
        assert_eq!(chip.bytes()[0x3F], 1);
        assert_eq!(chip.bytes()[0x20], 2);
        assert_eq!(chip.bytes()[0x40], ERASED);
    }

    #[test]
    fn eeprom_64k_page_writes_wrap_inside_128_bytes() {
        let mut chip = Backup::new(SaveType::Eeprom64K);
        write(&mut chip, WRITE, 0xFF7F, &[1, 2]);
        assert_eq!(chip.bytes()[0xFF7F], 1);
        assert_eq!(chip.bytes()[0xFF00], 2);
    }

    #[test]
    fn eeprom_64k_reads_wrap_at_the_end() {
        let mut chip = with_bytes(SaveType::Eeprom64K);
        assert_eq!(read(&mut chip, READ, 0xFFFF, 2), [(0xFFFF % 251) as u8, 0]);
    }

    #[test]
    fn eeprom_128k_uses_three_address_bytes() {
        let mut chip = Backup::new(SaveType::Eeprom128K);
        write(&mut chip, WRITE, 0x1_2345, &[0x99]);
        assert_eq!(chip.bytes()[0x1_2345], 0x99);
        assert_eq!(read(&mut chip, READ, 0x1_2345, 1), [0x99]);
    }

    #[test]
    fn write_status_sets_protect_and_lock_bits() {
        let mut chip = Backup::new(SaveType::Eeprom64K);
        command(&mut chip, &[WRITE_ENABLE]);
        command(&mut chip, &[WRITE_STATUS, 0xFF]);
        assert_eq!(status(&mut chip), 0x8C);
    }

    #[test]
    fn write_status_needs_write_enable() {
        let mut chip = Backup::new(SaveType::Eeprom64K);
        command(&mut chip, &[WRITE_STATUS, 0x0C]);
        assert_eq!(status(&mut chip), 0);
    }

    #[test]
    fn protect_bits_guard_a_quarter_a_half_or_all() {
        for (bits, first_protected) in [(1, 0x1800), (2, 0x1000), (3, 0)] {
            let mut chip = Backup::new(SaveType::Eeprom8K);
            command(&mut chip, &[WRITE_ENABLE]);
            command(&mut chip, &[WRITE_STATUS, bits << 2]);
            write(&mut chip, WRITE, first_protected, &[0x11]);
            assert_eq!(chip.bytes()[first_protected], ERASED, "bits {bits}");
            if first_protected > 0 {
                write(&mut chip, WRITE, first_protected - 1, &[0x22]);
                assert_eq!(chip.bytes()[first_protected - 1], 0x22, "bits {bits}");
            }
        }
    }

    #[test]
    fn eeprom_read_id_returns_ff() {
        let mut chip = Backup::new(SaveType::Eeprom8K);
        assert_eq!(command(&mut chip, &[READ_ID, 0, 0, 0])[1..], [0xFF; 3]);
    }

    #[test]
    fn a_cloned_save_keeps_its_bytes_when_the_original_writes_loads_or_erases() {
        let mut chip = Backup::new(SaveType::Flash256K);
        chip.load(&[7; 0x200]);
        let held = chip.clone();
        write(&mut chip, PAGE_WRITE, 0x10, &[1, 2, 3]);
        let written = chip.clone();
        write(&mut chip, PAGE_ERASE, 0, &[]);
        let erased = chip.clone();
        chip.load(&[9; 4]);
        assert_eq!(held.bytes()[..0x200], [7; 0x200]);
        assert_eq!(written.bytes()[0x10..0x13], [1, 2, 3]);
        assert_eq!(erased.bytes()[0xFF..0x101], [ERASED, 7]);
        assert_eq!(chip.bytes()[..5], [9, 9, 9, 9, ERASED]);
    }

    #[test]
    fn fram_writes_run_past_32_byte_pages() {
        let mut chip = Backup::new(SaveType::Fram32K);
        let data: Vec<u8> = (0..100).collect();
        write(&mut chip, WRITE, 0x10, &data);
        assert_eq!(chip.bytes()[0x10..0x74], data[..]);
    }

    #[test]
    fn fram_writes_wrap_at_the_end_of_the_chip() {
        let mut chip = Backup::new(SaveType::Fram8K);
        write(&mut chip, WRITE, 0x1FFF, &[1, 2]);
        assert_eq!((chip.bytes()[0x1FFF], chip.bytes()[0]), (1, 2));
    }

    #[test]
    fn flash_read_id_names_each_size() {
        for (kind, id) in [
            (SaveType::Flash256K, [0x20, 0x40, 0x12]),
            (SaveType::Flash512K, [0x20, 0x40, 0x13]),
            (SaveType::Flash1M, [0x20, 0x40, 0x14]),
            (SaveType::Flash8M, [0xC2, 0x20, 0x17]),
        ] {
            let mut chip = Backup::new(kind);
            assert_eq!(command(&mut chip, &[READ_ID, 0, 0, 0])[1..], id, "{kind:?}");
        }
    }

    #[test]
    fn flash_reads_with_three_address_bytes() {
        let mut chip = with_bytes(SaveType::Flash512K);
        assert_eq!(
            read(&mut chip, READ, 0x7_FFFF, 2),
            [(0x7_FFFF % 251) as u8, 0]
        );
    }

    #[test]
    fn flash_fast_read_skips_a_dummy_byte() {
        let mut chip = with_bytes(SaveType::Flash512K);
        let out = command(&mut chip, &[FAST_READ, 0x00, 0x01, 0x00, 0x00, 0, 0]);
        assert_eq!(out[5..], [(0x100 % 251) as u8, (0x101 % 251) as u8]);
    }

    #[test]
    fn flash_page_write_replaces_bytes() {
        let mut chip = with_bytes(SaveType::Flash512K);
        write(&mut chip, PAGE_WRITE, 0x1234, &[0xF0, 0x0F]);
        assert_eq!(chip.bytes()[0x1234..0x1236], [0xF0, 0x0F]);
        assert!(chip.dirty());
    }

    #[test]
    fn flash_page_write_wraps_inside_256_bytes() {
        let mut chip = Backup::new(SaveType::Flash256K);
        write(&mut chip, PAGE_WRITE, 0x2FF, &[1, 2]);
        assert_eq!((chip.bytes()[0x2FF], chip.bytes()[0x200]), (1, 2));
        assert_eq!(chip.bytes()[0x300], ERASED);
    }

    #[test]
    fn flash_page_program_only_clears_bits() {
        let mut chip = Backup::new(SaveType::Flash512K);
        write(&mut chip, WRITE, 0x40, &[0xF0]);
        write(&mut chip, WRITE, 0x40, &[0x3C]);
        assert_eq!(chip.bytes()[0x40], 0x30);
    }

    #[test]
    fn flash_page_erase_clears_256_bytes() {
        let mut chip = with_bytes(SaveType::Flash512K);
        write(&mut chip, PAGE_ERASE, 0x1234, &[]);
        assert!(
            chip.bytes()[0x1200..0x1300]
                .iter()
                .all(|&byte| byte == ERASED)
        );
        assert_eq!(chip.bytes()[0x11FF], (0x11FF % 251) as u8);
        assert_eq!(chip.bytes()[0x1300], (0x1300 % 251) as u8);
    }

    #[test]
    fn flash_sector_erase_clears_64k() {
        let mut chip = with_bytes(SaveType::Flash1M);
        write(&mut chip, SECTOR_ERASE, 0x2_3456, &[]);
        assert!(
            chip.bytes()[0x2_0000..0x3_0000]
                .iter()
                .all(|&byte| byte == ERASED)
        );
        assert_eq!(chip.bytes()[0x1_FFFF], (0x1_FFFF % 251) as u8);
        assert_eq!(chip.bytes()[0x3_0000], (0x3_0000 % 251) as u8);
    }

    #[test]
    fn flash_chip_erase_clears_everything() {
        let mut chip = with_bytes(SaveType::Flash256K);
        command(&mut chip, &[WRITE_ENABLE]);
        command(&mut chip, &[CHIP_ERASE]);
        assert!(chip.bytes().iter().all(|&byte| byte == ERASED));
    }

    #[test]
    fn flash_erase_needs_write_enable() {
        let mut chip = with_bytes(SaveType::Flash512K);
        command(&mut chip, &[PAGE_ERASE, 0x00, 0x00, 0x00]);
        command(&mut chip, &[CHIP_ERASE]);
        assert_eq!(chip.bytes()[1], 1);
        assert!(!chip.dirty());
    }

    #[test]
    fn flash_erase_clears_the_write_enable_latch() {
        let mut chip = with_bytes(SaveType::Flash512K);
        write(&mut chip, PAGE_ERASE, 0, &[]);
        assert_eq!(status(&mut chip), 0);
    }

    #[test]
    fn flash_status_holds_only_the_latch() {
        let mut chip = Backup::new(SaveType::Flash512K);
        command(&mut chip, &[WRITE_ENABLE]);
        command(&mut chip, &[WRITE_STATUS, 0xFC]);
        assert_eq!(status(&mut chip), STATUS_WRITE_ENABLED);
    }

    #[test]
    fn flash_sleep_ignores_all_but_wake() {
        let mut chip = with_bytes(SaveType::Flash512K);
        command(&mut chip, &[SLEEP]);
        assert_eq!(read(&mut chip, READ, 1, 1), [0xFF]);
        assert_eq!(status(&mut chip), 0xFF);
        command(&mut chip, &[WAKE]);
        assert_eq!(read(&mut chip, READ, 1, 1), [1]);
    }

    #[test]
    fn reads_do_not_mark_the_save_dirty() {
        let mut chip = with_bytes(SaveType::Fram32K);
        read(&mut chip, READ, 0, 16);
        assert!(!chip.dirty());
        write(&mut chip, WRITE, 0, &[0]);
        assert!(!chip.dirty());
        write(&mut chip, WRITE, 0, &[9]);
        assert!(chip.dirty());
        chip.clear_dirty();
        assert!(!chip.dirty());
    }

    #[test]
    fn unknown_commands_answer_ff() {
        let mut chip = with_bytes(SaveType::Eeprom64K);
        assert_eq!(command(&mut chip, &[0x77, 0, 0]), [0xFF; 3]);
    }

    #[test]
    fn the_save_type_comes_from_the_game_code() {
        assert_eq!(listing(*b"IPKE"), Listing::Chip(SaveType::Flash512K));
        assert_eq!(listing(*b"AMCE"), Listing::Chip(SaveType::Flash256K));
        assert_eq!(listing(*b"####"), Listing::Unlisted);
        assert_eq!(SAVE_TYPES.len() % SAVE_TYPE_ENTRY, 0);
    }

    #[test]
    fn a_game_listed_without_a_chip_has_no_save() {
        let entries = SAVE_TYPES.as_chunks::<SAVE_TYPE_ENTRY>().0;
        let row = entries
            .iter()
            .find(|row| row[4] == 0)
            .expect("a game without a save");
        assert_eq!(listing([row[0], row[1], row[2], row[3]]), Listing::NoSave);
    }

    #[test]
    fn a_save_file_size_picks_the_smallest_chip_that_holds_it() {
        assert_eq!(SaveType::for_size(0x200), Some(SaveType::Eeprom512));
        assert_eq!(SaveType::for_size(0x2000), Some(SaveType::Eeprom8K));
        assert_eq!(SaveType::for_size(0x8000), Some(SaveType::Fram32K));
        assert_eq!(SaveType::for_size(0x1_0000), Some(SaveType::Eeprom64K));
        assert_eq!(SaveType::for_size(0x2_0000), Some(SaveType::Eeprom128K));
        assert_eq!(SaveType::for_size(0x8_0000), Some(SaveType::Flash512K));
        assert_eq!(SaveType::for_size(0x7_FF00), Some(SaveType::Flash512K));
        assert_eq!(SaveType::for_size(0), None);
        assert_eq!(SaveType::for_size(0x80_0001), None);
    }
}

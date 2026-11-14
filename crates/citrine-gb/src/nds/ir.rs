mod link;

use super::save::Backup;
pub use link::{IrLink, airtime};

const PASS_THROUGH: u8 = 0x00;
const RECEIVE: u8 = 0x01;
const SEND: u8 = 0x02;
const WRITE_BYTE: u8 = 0x03;
const READ_BYTE: u8 = 0x04;
const WRITE_HALF: u8 = 0x05;
const READ_HALF: u8 = 0x06;
const ECHO: u8 = 0x07;
const VERSION: u8 = 0x08;
const NEW_CHIP_ID: u8 = 0xAA;
const IDLE_OUTPUT: u8 = 0x00;
const ADDRESS_BYTES: usize = 2;
const ECHO_SKIP: usize = 1;
const MEMORY_SIZE: usize = 0x1_0000;
const IR_CODE: u8 = b'I';
const FIRST_NEW_SECOND_CHAR: u8 = b'P';
const OLD_PACKET_LIMIT: usize = 0x84;
const NEW_PACKET_LIMIT: usize = 0xB8;
const OLD_REFUSED_FIRST: u8 = 0xF2;

pub trait IrPort {
    fn send(&mut self, packet: &[u8], at: u64);
    fn take(&mut self, now: u64) -> Option<Vec<u8>>;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct NoLink;

impl IrPort for NoLink {
    fn send(&mut self, _packet: &[u8], _at: u64) {}

    fn take(&mut self, _now: u64) -> Option<Vec<u8>> {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum IrVersion {
    Old,
    New,
}

impl IrVersion {
    pub fn for_game_code(code: [u8; 4]) -> Option<Self> {
        if code[0] != IR_CODE {
            return None;
        }
        Some(if code[1] < FIRST_NEW_SECOND_CHAR {
            Self::Old
        } else {
            Self::New
        })
    }

    fn knows(self, byte: u8) -> bool {
        match self {
            Self::Old => byte < VERSION,
            Self::New => true,
        }
    }

    fn packet_limit(self) -> usize {
        match self {
            Self::Old => OLD_PACKET_LIMIT,
            Self::New => NEW_PACKET_LIMIT,
        }
    }

    fn sends(self, packet: &[u8]) -> bool {
        match self {
            Self::Old => packet
                .first()
                .is_some_and(|&byte| byte != OLD_REFUSED_FIRST),
            Self::New => !packet.is_empty(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
enum Command {
    Waiting,
    PassThrough,
    Receive,
    Send,
    Memory { write: bool, width: usize },
    Echo,
    Version,
    Ignored,
}

impl Command {
    fn decode(byte: u8) -> Self {
        match byte {
            PASS_THROUGH => Self::PassThrough,
            RECEIVE => Self::Receive,
            SEND => Self::Send,
            WRITE_BYTE => Self::Memory {
                write: true,
                width: 1,
            },
            READ_BYTE => Self::Memory {
                write: false,
                width: 1,
            },
            WRITE_HALF => Self::Memory {
                write: true,
                width: 2,
            },
            READ_HALF => Self::Memory {
                write: false,
                width: 2,
            },
            ECHO => Self::Echo,
            VERSION => Self::Version,
            _ => Self::Ignored,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct IrChip {
    version: IrVersion,
    save: Backup,
    #[cfg_attr(feature = "serde", serde(with = "crate::utils::bytes"))]
    memory: Box<[u8]>,
    command: Command,
    parameters: Vec<u8>,
    last_parameters: Vec<u8>,
    received: Vec<u8>,
}

impl IrChip {
    pub fn new(version: IrVersion, save: Backup) -> Self {
        Self {
            version,
            save,
            memory: vec![0; MEMORY_SIZE].into_boxed_slice(),
            command: Command::Waiting,
            parameters: Vec::new(),
            last_parameters: Vec::new(),
            received: Vec::new(),
        }
    }

    pub fn version(&self) -> IrVersion {
        self.version
    }

    pub fn save(&self) -> &Backup {
        &self.save
    }

    pub fn save_mut(&mut self) -> &mut Backup {
        &mut self.save
    }

    pub fn transfer(&mut self, byte: u8, port: &mut dyn IrPort, now: u64) -> u8 {
        if self.command == Command::Waiting {
            if self.version.knows(byte) {
                self.command = Command::decode(byte);
            }
            if self.command == Command::Receive {
                self.received = port.take(now).unwrap_or_default();
                self.received.truncate(self.version.packet_limit());
            }
            return IDLE_OUTPUT;
        }
        if self.command == Command::PassThrough {
            return self.save.transfer(byte);
        }
        let position = self.parameters.len();
        self.parameters.push(byte);
        match self.command {
            Command::Receive => self.received_byte(position),
            Command::Memory { write, width } => self.memory_byte(write, width, position, byte),
            Command::Echo => position
                .checked_sub(ECHO_SKIP)
                .and_then(|index| self.last_parameters.get(index))
                .copied()
                .unwrap_or(IDLE_OUTPUT),
            Command::Version if position == 0 => NEW_CHIP_ID,
            _ => IDLE_OUTPUT,
        }
    }

    pub fn deselect(&mut self, port: &mut dyn IrPort, now: u64) {
        match self.command {
            Command::Waiting => return,
            Command::PassThrough => self.save.deselect(),
            _ => self.last_parameters = std::mem::take(&mut self.parameters),
        }
        if self.command == Command::Send {
            let packet = &self.last_parameters
                [..self.last_parameters.len().min(self.version.packet_limit())];
            if self.version.sends(packet) {
                port.send(packet, now);
            }
        }
        self.received.clear();
        self.parameters.clear();
        self.command = Command::Waiting;
    }

    fn received_byte(&self, position: usize) -> u8 {
        match position.checked_sub(1) {
            None => self.received.len() as u8,
            Some(index) => self.received.get(index).copied().unwrap_or(IDLE_OUTPUT),
        }
    }

    fn memory_byte(&mut self, write: bool, width: usize, position: usize, byte: u8) -> u8 {
        let Some(offset) = position.checked_sub(ADDRESS_BYTES) else {
            return IDLE_OUTPUT;
        };
        if offset >= width {
            return IDLE_OUTPUT;
        }
        let base = usize::from(u16::from_be_bytes([self.parameters[0], self.parameters[1]]));
        let address = (base + offset) % MEMORY_SIZE;
        if write {
            self.memory[address] = byte;
            IDLE_OUTPUT
        } else {
            self.memory[address]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nds::save::SaveType;

    const WRITE_ENABLE: u8 = 0x06;
    const WRITE: u8 = 0x02;
    const READ: u8 = 0x03;
    const READ_STATUS: u8 = 0x05;

    fn plain(save: &mut Backup, bytes: &[u8]) -> Vec<u8> {
        let out = bytes.iter().map(|&byte| save.transfer(byte)).collect();
        save.deselect();
        out
    }

    fn through(chip: &mut IrChip, bytes: &[u8]) -> Vec<u8> {
        let out = command(chip, &[&[PASS_THROUGH], bytes].concat());
        out[1..].to_vec()
    }

    fn command(chip: &mut IrChip, bytes: &[u8]) -> Vec<u8> {
        let out = bytes
            .iter()
            .map(|&byte| chip.transfer(byte, &mut NoLink, 0))
            .collect();
        chip.deselect(&mut NoLink, 0);
        out
    }

    fn new_chip() -> IrChip {
        IrChip::new(IrVersion::New, Backup::new(SaveType::Flash512K))
    }

    fn old_chip() -> IrChip {
        IrChip::new(IrVersion::Old, Backup::new(SaveType::Eeprom64K))
    }

    fn session(address_bytes: usize) -> Vec<Vec<u8>> {
        let at = |address: u32| address.to_be_bytes()[4 - address_bytes..].to_vec();
        vec![
            vec![WRITE_ENABLE],
            vec![READ_STATUS, 0],
            [&[WRITE][..], &at(0x0123), &[0xDE, 0xAD, 0xBE, 0xEF]].concat(),
            vec![READ_STATUS, 0],
            [&[READ][..], &at(0x0122), &[0; 6]].concat(),
            vec![WRITE_ENABLE],
            [&[WRITE][..], &at(0xFFFE), &[1, 2, 3, 4]].concat(),
            [&[READ][..], &at(0xFFFC), &[0; 8]].concat(),
        ]
    }

    #[test]
    fn pass_through_saves_match_the_bare_chip_byte_for_byte() {
        for (kind, address_bytes) in [(SaveType::Eeprom64K, 2), (SaveType::Flash512K, 3)] {
            let mut bare = Backup::new(kind);
            let mut chip = IrChip::new(IrVersion::New, Backup::new(kind));
            for bytes in session(address_bytes) {
                assert_eq!(through(&mut chip, &bytes), plain(&mut bare, &bytes));
            }
            assert_eq!(chip.save().bytes(), bare.bytes());
            assert!(chip.save().dirty());
        }
    }

    #[test]
    fn a_save_loaded_before_the_chip_reads_back_through_it() {
        let mut save = Backup::new(SaveType::Flash512K);
        let bytes: Vec<u8> = (0..SaveType::Flash512K.size())
            .map(|i| (i % 253) as u8)
            .collect();
        save.load(&bytes);
        let mut chip = IrChip::new(IrVersion::New, save);
        let read = through(&mut chip, &[READ, 0x01, 0x00, 0x10, 0, 0, 0]);
        assert_eq!(&read[4..], &bytes[0x1_0010..0x1_0013]);
    }

    #[test]
    fn the_command_byte_answers_zero() {
        assert_eq!(new_chip().transfer(PASS_THROUGH, &mut NoLink, 0), 0);
    }

    #[test]
    fn the_new_chip_answers_its_version_once() {
        assert_eq!(command(&mut new_chip(), &[VERSION, 0, 0]), [0, 0xAA, 0]);
    }

    #[test]
    fn the_new_chip_pads_unknown_commands_with_zero() {
        let mut chip = new_chip();
        assert_eq!(command(&mut chip, &[0x09, 0xFF, 0xFF]), [0, 0, 0]);
        assert_eq!(
            command(&mut chip, &[0xFF, PASS_THROUGH, READ_STATUS, 0]),
            [0; 4]
        );
    }

    #[test]
    fn the_old_chip_skips_unknown_bytes_until_a_command() {
        let mut chip = old_chip();
        let mut wire = NoLink;
        chip.transfer(VERSION, &mut wire, 0);
        chip.transfer(0xF2, &mut wire, 0);
        chip.transfer(PASS_THROUGH, &mut wire, 0);
        assert_eq!(chip.transfer(WRITE_ENABLE, &mut wire, 0), 0xFF);
        chip.deselect(&mut wire, 0);
        assert_eq!(through(&mut chip, &[READ_STATUS, 0])[1], 0x02);
    }

    #[test]
    fn ir_commands_leave_the_save_chip_alone() {
        let mut chip = new_chip();
        through(&mut chip, &[WRITE_ENABLE]);
        command(&mut chip, &[VERSION, WRITE, 0, 0, 0x55]);
        command(&mut chip, &[WRITE_BYTE, 0, 0, 0x55]);
        assert_eq!(through(&mut chip, &[READ_STATUS, 0])[1], 0x02);
        assert!(!chip.save().dirty());
    }

    #[test]
    fn byte_writes_read_back_at_their_address() {
        for mut chip in [new_chip(), old_chip()] {
            command(&mut chip, &[WRITE_BYTE, 0xFE, 0x44, 0x5A]);
            assert_eq!(
                command(&mut chip, &[READ_BYTE, 0xFE, 0x44, 0]),
                [0, 0, 0, 0x5A]
            );
            assert_eq!(command(&mut chip, &[READ_BYTE, 0xFE, 0x45, 0]), [0; 4]);
        }
    }

    #[test]
    fn half_writes_cover_two_bytes_high_first() {
        let mut chip = new_chip();
        command(&mut chip, &[WRITE_HALF, 0xFF, 0xD6, 0x12, 0x34, 0x99]);
        assert_eq!(
            command(&mut chip, &[READ_BYTE, 0xFF, 0xD6, 0]),
            [0, 0, 0, 0x12]
        );
        assert_eq!(
            command(&mut chip, &[READ_HALF, 0xFF, 0xD6, 0, 0, 0]),
            [0, 0, 0, 0x12, 0x34, 0]
        );
    }

    #[test]
    fn echo_returns_the_last_commands_parameters() {
        let mut chip = new_chip();
        command(&mut chip, &[WRITE_BYTE, 0x80, 0x01, 0x77]);
        assert_eq!(
            command(&mut chip, &[ECHO, 0, 0, 0, 0, 0]),
            [0, 0, 0x80, 0x01, 0x77, 0]
        );
        assert_eq!(command(&mut chip, &[ECHO, 0, 0, 0]), [0, 0, 0, 0]);
    }

    #[test]
    fn heartgold_soulsilver_and_gen_5_use_the_new_chip() {
        for code in [
            b"IPKE", b"IPGE", b"IRBO", b"IRAO", b"IREO", b"IRDO", b"IPKJ",
        ] {
            assert_eq!(IrVersion::for_game_code(*code), Some(IrVersion::New));
        }
    }

    #[test]
    fn ir_codes_below_p_use_the_old_chip() {
        for code in [b"IAAA", b"IMWE", b"IOZZ"] {
            assert_eq!(IrVersion::for_game_code(*code), Some(IrVersion::Old));
        }
    }

    #[test]
    fn other_games_have_no_ir_chip() {
        for code in [b"ADAE", b"CPUE", b"APAE", b"\0\0\0\0", b"iPKE"] {
            assert_eq!(IrVersion::for_game_code(*code), None);
        }
    }
}

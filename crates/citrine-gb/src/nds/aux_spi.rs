use super::ir::{IrChip, IrPort, IrVersion};
use super::save::{self, Backup, Listing, SaveType};
use std::collections::VecDeque;

const HIGH_Z: u8 = 0xFF;
const WRITE: u8 = 0x02;
const READ: u8 = 0x03;
const READ_STATUS: u8 = 0x05;
const WRITE_HIGH: u8 = 0x0A;
const READ_HIGH: u8 = 0x0B;
const GUESS_STATUS: u8 = 0x00;
const GUESS_MEMORY: usize = 8;
const MAILBOX_LIMIT: usize = 16;
const IR_FALLBACK: SaveType = SaveType::Flash512K;

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AuxDevice {
    #[default]
    Empty,
    Save(Backup),
    Guess(Guess),
    Ir(Infrared),
}

impl AuxDevice {
    pub fn for_game(game_code: [u8; 4]) -> Self {
        let listing = save::listing(game_code);
        if let Some(version) = IrVersion::for_game_code(game_code) {
            let kind = match listing {
                Listing::Chip(kind) => kind,
                Listing::NoSave | Listing::Unlisted => IR_FALLBACK,
            };
            return Self::Ir(Infrared::new(IrChip::new(version, Backup::new(kind))));
        }
        match listing {
            Listing::Chip(kind) => Self::Save(Backup::new(kind)),
            Listing::NoSave => Self::Empty,
            Listing::Unlisted => Self::Guess(Guess::default()),
        }
    }

    pub fn backup(&self) -> Option<&Backup> {
        match self {
            Self::Save(backup) => Some(backup),
            Self::Ir(ir) => Some(ir.chip.save()),
            Self::Empty | Self::Guess(_) => None,
        }
    }

    pub fn backup_mut(&mut self) -> Option<&mut Backup> {
        match self {
            Self::Save(backup) => Some(backup),
            Self::Ir(ir) => Some(ir.chip.save_mut()),
            Self::Empty | Self::Guess(_) => None,
        }
    }

    pub fn has_infrared(&self) -> bool {
        matches!(self, Self::Ir(_))
    }

    pub fn has_save(&self) -> bool {
        !matches!(self, Self::Empty)
    }

    pub fn load(&mut self, bytes: &[u8]) {
        if let Self::Guess(_) = self {
            match SaveType::for_size(bytes.len()) {
                Some(kind) => *self = Self::Save(Backup::new(kind)),
                None => return,
            }
        }
        if let Some(backup) = self.backup_mut() {
            backup.load(bytes);
        }
    }

    pub fn transfer(&mut self, byte: u8, now: u64) -> u8 {
        match self {
            Self::Empty => HIGH_Z,
            Self::Save(backup) => backup.transfer(byte),
            Self::Guess(guess) => guess.transfer(byte),
            Self::Ir(ir) => ir.chip.transfer(byte, &mut ir.mailbox, now),
        }
    }

    pub fn deselect(&mut self, now: u64) {
        match self {
            Self::Empty => {}
            Self::Save(backup) => backup.deselect(),
            Self::Guess(guess) => {
                if let Some(backup) = guess.deselect() {
                    *self = Self::Save(backup);
                }
            }
            Self::Ir(ir) => ir.chip.deselect(&mut ir.mailbox, now),
        }
    }

    pub fn exchange(&mut self, port: &mut dyn IrPort, now: u64) {
        if let Self::Ir(ir) = self {
            ir.mailbox.exchange(port, now);
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Guess {
    selections: VecDeque<Vec<u8>>,
    current: Vec<u8>,
}

impl Guess {
    fn transfer(&mut self, byte: u8) -> u8 {
        self.current.push(byte);
        if self.current.len() > 1 && self.current[0] == READ_STATUS {
            GUESS_STATUS
        } else {
            HIGH_Z
        }
    }

    fn deselect(&mut self) -> Option<Backup> {
        let bytes = std::mem::take(&mut self.current);
        if bytes.is_empty() {
            return None;
        }
        let kind = guess(&bytes);
        self.selections.push_back(bytes);
        if self.selections.len() > GUESS_MEMORY {
            self.selections.pop_front();
        }
        let mut backup = Backup::new(kind?);
        for selection in &self.selections {
            for &byte in selection {
                backup.transfer(byte);
            }
            backup.deselect();
        }
        Some(backup)
    }
}

fn guess(bytes: &[u8]) -> Option<SaveType> {
    let (&command, rest) = bytes.split_first()?;
    if ![READ, WRITE, READ_HIGH, WRITE_HIGH].contains(&command) {
        return None;
    }
    let address_bytes = match rest.len() {
        0..=2 => 1,
        3 => 2,
        4 => 3,
        n => n & 3,
    };
    match address_bytes {
        1 => Some(SaveType::Eeprom512),
        2 => Some(SaveType::Eeprom64K),
        3 => Some(SaveType::Flash512K),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Infrared {
    chip: IrChip,
    mailbox: Mailbox,
}

impl Infrared {
    fn new(chip: IrChip) -> Self {
        Self {
            chip,
            mailbox: Mailbox::default(),
        }
    }

    pub fn chip(&self) -> &IrChip {
        &self.chip
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Mailbox {
    outbox: VecDeque<(Vec<u8>, u64)>,
    inbox: VecDeque<Vec<u8>>,
}

impl Mailbox {
    fn exchange(&mut self, port: &mut dyn IrPort, now: u64) {
        while let Some((packet, at)) = self.outbox.pop_front() {
            port.send(&packet, at);
        }
        while let Some(packet) = port.take(now) {
            push_bounded(&mut self.inbox, packet);
        }
    }
}

impl IrPort for Mailbox {
    fn send(&mut self, packet: &[u8], at: u64) {
        push_bounded(&mut self.outbox, (packet.to_vec(), at));
    }

    fn take(&mut self, _now: u64) -> Option<Vec<u8>> {
        self.inbox.pop_front()
    }
}

fn push_bounded<T>(queue: &mut VecDeque<T>, item: T) {
    if queue.len() == MAILBOX_LIMIT {
        queue.pop_front();
    }
    queue.push_back(item);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nds::ir::IrLink;

    const WRITE_ENABLE: u8 = 0x06;

    fn command(device: &mut AuxDevice, bytes: &[u8]) -> Vec<u8> {
        let out = bytes.iter().map(|&byte| device.transfer(byte, 0)).collect();
        device.deselect(0);
        out
    }

    fn kind(device: &AuxDevice) -> Option<SaveType> {
        device.backup().map(Backup::kind)
    }

    #[test]
    fn a_listed_game_gets_its_chip() {
        let device = AuxDevice::for_game(*b"AMCE");
        assert_eq!(kind(&device), Some(SaveType::Flash256K));
    }

    #[test]
    fn heartgold_gets_the_ir_chip_in_front_of_its_flash() {
        let AuxDevice::Ir(ir) = AuxDevice::for_game(*b"IPKE") else {
            panic!("no ir chip");
        };
        assert_eq!(ir.chip().version(), IrVersion::New);
        assert_eq!(ir.chip().save().kind(), SaveType::Flash512K);
    }

    #[test]
    fn an_unlisted_game_guesses_from_its_first_read() {
        for (address, expected) in [
            (&[0x12][..], SaveType::Eeprom512),
            (&[0x00, 0x12], SaveType::Eeprom64K),
            (&[0x00, 0x00, 0x12], SaveType::Flash512K),
        ] {
            let mut device = AuxDevice::for_game(*b"####");
            assert_eq!(kind(&device), None);
            let read = [&[READ][..], address, &[0]].concat();
            assert_eq!(command(&mut device, &read), vec![HIGH_Z; read.len()]);
            assert_eq!(kind(&device), Some(expected));
        }
    }

    #[test]
    fn a_guessed_chip_keeps_the_write_that_decided_it() {
        let mut device = AuxDevice::for_game(*b"####");
        command(&mut device, &[READ_STATUS, 0]);
        command(&mut device, &[WRITE_ENABLE]);
        command(&mut device, &[WRITE, 0x00, 0x10, 0xAB]);
        assert_eq!(kind(&device), Some(SaveType::Eeprom64K));
        assert_eq!(device.backup().unwrap().bytes()[0x10], 0xAB);
        assert!(device.backup().unwrap().dirty());
        assert_eq!(
            command(&mut device, &[READ, 0x00, 0x10, 0]),
            [HIGH_Z, HIGH_Z, HIGH_Z, 0xAB]
        );
    }

    #[test]
    fn the_status_reads_zero_before_the_guess() {
        let mut device = AuxDevice::for_game(*b"####");
        assert_eq!(command(&mut device, &[READ_STATUS, 0, 0]), [HIGH_Z, 0, 0]);
        assert_eq!(kind(&device), None);
    }

    #[test]
    fn a_save_file_decides_the_chip_of_an_unlisted_game() {
        let mut device = AuxDevice::for_game(*b"####");
        device.load(&vec![0x5A; 0x2000]);
        assert_eq!(kind(&device), Some(SaveType::Eeprom8K));
        assert_eq!(
            command(&mut device, &[READ, 0x1F, 0xFF, 0]),
            [HIGH_Z, HIGH_Z, HIGH_Z, 0x5A]
        );
    }

    #[test]
    fn a_save_file_keeps_the_listed_chip() {
        let mut device = AuxDevice::for_game(*b"AMCE");
        device.load(&[0x11; 0x2000]);
        assert_eq!(kind(&device), Some(SaveType::Flash256K));
        assert_eq!(device.backup().unwrap().bytes()[0x1FFF], 0x11);
        assert_eq!(device.backup().unwrap().bytes()[0x2000], 0xFF);
    }

    #[test]
    fn a_game_without_a_save_ignores_the_bus_and_save_files() {
        let mut device = AuxDevice::Empty;
        device.load(&[0; 0x200]);
        assert_eq!(command(&mut device, &[READ, 0, 0]), [HIGH_Z; 3]);
        assert!(!device.has_save());
    }

    #[test]
    fn ir_packets_wait_in_the_mailbox_until_the_exchange() {
        let mut device = AuxDevice::for_game(*b"IPKE");
        let mut link = IrLink::new();
        let at = 1_000;
        for byte in [0x02, 0xAA, 0xBB] {
            device.transfer(byte, at);
        }
        device.deselect(at);
        let later = at + 10 * crate::nds::ir::airtime(2);
        assert_eq!(link.end(1).take(later), None);
        device.exchange(&mut link.end(0), later);
        assert_eq!(link.end(1).take(later), Some(vec![0xAA, 0xBB]));
        link.end(1).send(&[0x56], later);
        let answer = later + 10 * crate::nds::ir::airtime(1);
        device.exchange(&mut link.end(0), answer);
        let received = [0x01, 0, 0]
            .into_iter()
            .map(|byte| device.transfer(byte, answer))
            .collect::<Vec<_>>();
        device.deselect(answer);
        assert_eq!(received, [0x00, 1, 0x56]);
    }

    #[test]
    fn an_unlinked_mailbox_holds_only_the_newest_packets() {
        let mut mailbox = Mailbox::default();
        for at in 0..40 {
            mailbox.send(&[at as u8], at);
        }
        assert_eq!(mailbox.outbox.len(), MAILBOX_LIMIT);
        assert_eq!(mailbox.outbox.front().unwrap().1, 40 - MAILBOX_LIMIT as u64);
    }
}

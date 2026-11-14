pub const EEPROM_SIZE: usize = 0x10000;

const MAGIC: &[u8; 8] = b"nintendo";
const FACTORY: usize = 0x0080;
const FIRST_SETTINGS: u8 = 0x24;
const COPY_OFFSET: usize = 0x100;

pub(crate) const RECEIVED: usize = 0xB800;
pub(crate) const SPECIAL_ROUTE: usize = 0xBF00;
pub(crate) const RECEIVED_SIZE: usize = 0x6C8;
pub(crate) const ROUTE: usize = 0x8F00;
pub(crate) const ROUTE_STAGING: usize = 0xD700;
pub(crate) const ROUTE_UPLOAD_SIZE: usize = 0x2900;
pub(crate) const TEAM: usize = 0xCC00;
pub(crate) const TEAM_STAGING: usize = 0xD480;
pub(crate) const TEAM_UPLOAD_SIZE: usize = 0x280;
pub(crate) const RESULTS: usize = 0xCE80;
pub(crate) const RESULTS_SIZE: usize = 0xD4C;
pub(crate) const WATTS: usize = 0xCE8A;
pub(crate) const CAUGHT: usize = 0xCE8C;
pub(crate) const CAUGHT_SIZE: usize = 0x64;
pub(crate) const EVENT_LOG: usize = 0xCF0C;
pub(crate) const PEERS: usize = 0xDE24;
pub(crate) const PEERS_SIZE: usize = 0x1568;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Block {
    UniqueIdentity,
    LcdCommands,
    Info,
    Health,
    CopyMarker,
}

impl Block {
    pub const ALL: [Block; 5] = [
        Block::UniqueIdentity,
        Block::LcdCommands,
        Block::Info,
        Block::Health,
        Block::CopyMarker,
    ];

    pub fn address(self) -> usize {
        match self {
            Block::UniqueIdentity => 0x0083,
            Block::LcdCommands => 0x00AC,
            Block::Info => 0x00ED,
            Block::Health => 0x0156,
            Block::CopyMarker => 0x016F,
        }
    }

    pub fn size(self) -> usize {
        match self {
            Block::UniqueIdentity => 0x28,
            Block::LcdCommands => 0x40,
            Block::Info => 0x68,
            Block::Health => 0x18,
            Block::CopyMarker => 1,
        }
    }

    fn copy(self, second: bool) -> usize {
        self.address() + if second { COPY_OFFSET } else { 0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repair {
    Intact,
    FromFirst,
    FromSecond,
    Lost,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Eeprom(Box<[u8; EEPROM_SIZE]>);

impl std::fmt::Debug for Eeprom {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Eeprom").finish_non_exhaustive()
    }
}

impl Default for Eeprom {
    fn default() -> Self {
        Self::blank()
    }
}

impl Eeprom {
    pub fn blank() -> Self {
        let mut eeprom = Self(Box::new([0xFF; EEPROM_SIZE]));
        eeprom.format();
        eeprom
    }

    pub(crate) fn format(&mut self) {
        self.write(0, MAGIC);
        let factory = [0, 0];
        let mut stored = factory.to_vec();
        stored.push(block_sum(&factory));
        self.write(FACTORY, &stored);
        self.write(FACTORY + COPY_OFFSET, &stored);
        let mut info = self.read(Block::Info.address(), Block::Info.size());
        info[..0x10].fill(0);
        info[0x38..0x5C].fill(0);
        info[0x5E..].fill(0);
        info[0x5F] = 2;
        self.write_reliable(Block::Info, &info);
        let mut health = [0; 0x18];
        health[0x08..0x0C].copy_from_slice(&crate::info::FIRST_CLOCK.to_be_bytes());
        health[0x17] = FIRST_SETTINGS;
        self.write_reliable(Block::Health, &health);
        self.fill(RECEIVED, RECEIVED_SIZE, 0);
        self.fill(RESULTS, RESULTS_SIZE, 0);
        self.fill(PEERS, PEERS_SIZE, 0);
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let image: Box<[u8; EEPROM_SIZE]> = bytes.to_vec().into_boxed_slice().try_into().ok()?;
        Some(Self(image))
    }

    pub fn bytes(&self) -> &[u8] {
        &self.0[..]
    }

    pub fn has_magic(&self) -> bool {
        &self.0[..MAGIC.len()] == MAGIC
    }

    pub fn read(&self, address: usize, len: usize) -> Vec<u8> {
        (0..len)
            .map(|i| self.0[(address + i) % EEPROM_SIZE])
            .collect()
    }

    pub fn write(&mut self, address: usize, data: &[u8]) {
        for (i, byte) in data.iter().enumerate() {
            self.0[(address + i) % EEPROM_SIZE] = *byte;
        }
    }

    pub fn fill(&mut self, address: usize, len: usize, byte: u8) {
        self.write(address, &vec![byte; len]);
    }

    pub fn copy_within(&mut self, from: usize, to: usize, len: usize) {
        let data = self.read(from, len);
        self.write(to, &data);
    }

    pub fn read_reliable(&mut self, block: Block) -> Option<Vec<u8>> {
        match self.repair(block) {
            Repair::Lost => None,
            _ => Some(self.read(block.address(), block.size())),
        }
    }

    pub fn write_reliable(&mut self, block: Block, data: &[u8]) {
        assert_eq!(data.len(), block.size());
        let mut stored = data.to_vec();
        stored.push(block_sum(data));
        self.write(block.copy(false), &stored);
        self.write(block.copy(true), &stored);
    }

    pub fn repair(&mut self, block: Block) -> Repair {
        let first = self.read(block.copy(false), block.size() + 1);
        let second = self.read(block.copy(true), block.size() + 1);
        let first_good = sum_holds(&first);
        let second_good = sum_holds(&second);
        if first_good && second_good && first.last() == second.last() {
            return Repair::Intact;
        }
        if first_good {
            self.write(block.copy(true), &first);
            return Repair::FromFirst;
        }
        if second_good {
            self.write(block.copy(false), &second);
            return Repair::FromSecond;
        }
        Repair::Lost
    }

    pub fn repair_all(&mut self) -> Vec<(Block, Repair)> {
        Block::ALL
            .into_iter()
            .map(|block| (block, self.repair(block)))
            .filter(|(_, repair)| *repair != Repair::Intact)
            .collect()
    }
}

pub(crate) fn block_sum(data: &[u8]) -> u8 {
    data.iter().fold(1u8, |sum, byte| sum.wrapping_add(*byte))
}

fn sum_holds(stored: &[u8]) -> bool {
    let (data, sum) = stored.split_at(stored.len() - 1);
    block_sum(data) == sum[0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blank_image_is_formatted_like_the_walkers_first_boot() {
        let mut eeprom = Eeprom::blank();
        assert_eq!(eeprom.read(0, 8), b"nintendo");
        assert_eq!(eeprom.read(0x0080, 4), vec![0, 0, 1, 0xFF]);
        assert_eq!(eeprom.read(0x00FD, 0x28), vec![0xFF; 0x28]);
        assert_eq!(
            eeprom.read(0x0149, 13),
            [0xFF, 0xFF, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0xD9]
        );
        assert_eq!(eeprom.read(0x015E, 4), vec![0x0D, 0x2B, 0x0B, 0x80]);
        assert_eq!(eeprom.read(0x016D, 3), vec![0x24, 0xE8, 0xFF]);
        assert_eq!(eeprom.read(0x0256, 0x19), eeprom.read(0x0156, 0x19));
        assert_eq!(eeprom.read(0x0280, 0x10), vec![0xFF; 0x10]);
        assert_eq!(eeprom.read(0xCE80, 0xD4C), vec![0; 0xD4C]);
        assert_eq!(eeprom.read(0xDE24 + 0x1568, 1), vec![0xFF]);
        assert_eq!(eeprom.repair(Block::Info), Repair::Intact);
        assert_eq!(eeprom.repair(Block::Health), Repair::Intact);
    }

    #[test]
    fn a_reliable_write_lands_in_both_copies_with_its_sum() {
        let mut eeprom = Eeprom::blank();
        let data: Vec<u8> = (0..0x18).collect();
        eeprom.write_reliable(Block::Health, &data);
        let sum = (0..0x18u8).fold(1u8, |sum, byte| sum.wrapping_add(byte));
        assert_eq!(eeprom.read(0x0156, 0x18), data);
        assert_eq!(eeprom.read(0x016E, 1), vec![sum]);
        assert_eq!(eeprom.read(0x0256, 0x18), data);
        assert_eq!(eeprom.read(0x026E, 1), vec![sum]);
    }

    #[test]
    fn the_sum_starts_at_one_and_wraps() {
        assert_eq!(block_sum(&[]), 1);
        assert_eq!(block_sum(&[0xFF]), 0);
        assert_eq!(block_sum(&[0x80, 0x80, 0x05]), 6);
    }

    #[test]
    fn a_corrupted_first_copy_is_repaired_from_its_twin() {
        let mut eeprom = Eeprom::blank();
        let info: Vec<u8> = (0..0x68).map(|i| i as u8 ^ 0x5A).collect();
        eeprom.write_reliable(Block::Info, &info);
        eeprom.write(0x00ED + 0x0C, &[0xEE]);
        assert_eq!(eeprom.read_reliable(Block::Info), Some(info.clone()));
        assert_eq!(eeprom.read(0x00ED, 0x69), eeprom.read(0x01ED, 0x69));
        assert_eq!(eeprom.repair(Block::Info), Repair::Intact);
    }

    #[test]
    fn a_corrupted_second_copy_is_repaired_from_the_first() {
        let mut eeprom = Eeprom::blank();
        eeprom.write_reliable(Block::Health, &[7; 0x18]);
        eeprom.write(0x026E, &[0]);
        assert_eq!(eeprom.repair(Block::Health), Repair::FromFirst);
        assert_eq!(eeprom.read(0x0256, 0x19), eeprom.read(0x0156, 0x19));
    }

    #[test]
    fn copies_that_differ_with_good_sums_follow_the_first() {
        let mut eeprom = Eeprom::blank();
        eeprom.write_reliable(Block::CopyMarker, &[0xA5]);
        eeprom.write(0x026F, &[0x00, 0x01]);
        assert_eq!(eeprom.repair(Block::CopyMarker), Repair::FromFirst);
        assert_eq!(eeprom.read(0x026F, 2), vec![0xA5, 0xA6]);
    }

    #[test]
    fn both_copies_bad_reads_as_lost() {
        let mut eeprom = Eeprom::blank();
        eeprom.write(0x0083, &[9]);
        eeprom.write(0x0183, &[9]);
        assert_eq!(eeprom.repair(Block::UniqueIdentity), Repair::Lost);
        assert_eq!(eeprom.read_reliable(Block::UniqueIdentity), None);
    }

    #[test]
    fn images_round_trip_through_bytes() {
        let mut eeprom = Eeprom::blank();
        eeprom.write(0xFFFE, &[1, 2]);
        let again = Eeprom::from_bytes(eeprom.bytes()).unwrap();
        assert_eq!(again, eeprom);
        assert!(Eeprom::from_bytes(&[0; 10]).is_none());
    }
}

use super::image::{HEADER_BYTES, RSA_BYTES};

pub const MAX_BINARY_BYTES: usize = 0x0040_0000;

const HEADER_SIZE_IN_RSA: usize = 0x14;
const ARM9_SIZE_IN_RSA: usize = 0x24;
const ARM7_SIZE_IN_RSA: usize = 0x34;
const SECTIONS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Transfer {
    packet_bytes: usize,
    rsa: Option<Vec<u8>>,
    sections: Vec<Vec<u8>>,
    received: Vec<bool>,
    got: usize,
    got_bytes: usize,
}

impl Transfer {
    pub fn new(packet_bytes: usize) -> Self {
        Self {
            packet_bytes: packet_bytes.max(1),
            rsa: None,
            sections: Vec::new(),
            received: Vec::new(),
            got: 0,
            got_bytes: 0,
        }
    }

    pub fn has_rsa(&self) -> bool {
        self.rsa.is_some()
    }

    pub fn take_rsa(&mut self, rsa: &[u8]) -> bool {
        if self.rsa.is_some() {
            return true;
        }
        let Some(rsa) = rsa.get(..RSA_BYTES) else {
            return false;
        };
        let sizes = [HEADER_SIZE_IN_RSA, ARM9_SIZE_IN_RSA, ARM7_SIZE_IN_RSA].map(|at| {
            u32::from_le_bytes([rsa[at], rsa[at + 1], rsa[at + 2], rsa[at + 3]]) as usize
        });
        if sizes[0] != HEADER_BYTES || sizes[1..].iter().any(|&size| size > MAX_BINARY_BYTES) {
            return false;
        }
        self.sections = sizes.iter().map(|&size| vec![0; size]).collect();
        let packets = sizes
            .iter()
            .map(|&size| size.div_ceil(self.packet_bytes))
            .sum();
        self.received = vec![false; packets];
        self.rsa = Some(rsa.to_vec());
        true
    }

    pub fn take_packet(&mut self, number: u16, data: &[u8]) -> bool {
        let number = usize::from(number);
        let Some((section, offset, length)) = self.place(number) else {
            return false;
        };
        if data.len() < length || data.len() > length + 1 {
            return false;
        }
        self.sections[section][offset..offset + length].copy_from_slice(&data[..length]);
        if !self.received[number] {
            self.received[number] = true;
            self.got += 1;
            self.got_bytes += length;
        }
        true
    }

    pub fn total(&self) -> usize {
        self.received.len()
    }

    pub fn got(&self) -> usize {
        self.got
    }

    pub fn bytes(&self) -> usize {
        self.sections.iter().map(Vec::len).sum()
    }

    pub fn got_bytes(&self) -> usize {
        self.got_bytes
    }

    pub fn next_missing(&self) -> u16 {
        self.received
            .iter()
            .position(|&got| !got)
            .unwrap_or(self.received.len()) as u16
    }

    pub fn complete(&self) -> bool {
        self.rsa.is_some() && self.got == self.received.len()
    }

    pub fn parts(&self) -> Option<[&[u8]; SECTIONS]> {
        if !self.complete() {
            return None;
        }
        Some([0, 1, 2].map(|section| self.sections[section].as_slice()))
    }

    fn place(&self, number: usize) -> Option<(usize, usize, usize)> {
        let mut first = 0;
        for (section, bytes) in self.sections.iter().enumerate() {
            let packets = bytes.len().div_ceil(self.packet_bytes);
            if number < first + packets {
                let offset = (number - first) * self.packet_bytes;
                return Some((section, offset, self.packet_bytes.min(bytes.len() - offset)));
            }
            first += packets;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rsa(arm9: u32, arm7: u32) -> Vec<u8> {
        let mut rsa = vec![0; RSA_BYTES];
        rsa[HEADER_SIZE_IN_RSA..HEADER_SIZE_IN_RSA + 4]
            .copy_from_slice(&(HEADER_BYTES as u32).to_le_bytes());
        rsa[ARM9_SIZE_IN_RSA..ARM9_SIZE_IN_RSA + 4].copy_from_slice(&arm9.to_le_bytes());
        rsa[ARM7_SIZE_IN_RSA..ARM7_SIZE_IN_RSA + 4].copy_from_slice(&arm7.to_le_bytes());
        rsa
    }

    #[test]
    fn each_section_is_cut_into_its_own_packets() {
        let mut transfer = Transfer::new(0x100);
        assert!(transfer.take_rsa(&rsa(0x250, 0x80)));
        assert_eq!(transfer.total(), 2 + 3 + 1);
        assert!(!transfer.take_packet(1, &[0; 0x100]));
        assert!(transfer.take_packet(1, &[1; 0x60]));
        assert!(transfer.take_packet(4, &[4; 0x50]));
        assert!(!transfer.take_packet(6, &[6; 0x10]));
        assert_eq!(transfer.next_missing(), 0);
        for (number, length) in [(0, 0x100), (2, 0x100), (3, 0x100), (5, 0x80)] {
            assert!(!transfer.complete());
            assert!(transfer.take_packet(number, &vec![number as u8; length]));
        }
        assert!(transfer.complete());
        let [header, arm9, arm7] = transfer.parts().unwrap();
        assert_eq!(header.len(), HEADER_BYTES);
        assert_eq!(header[0x100], 1);
        assert_eq!(arm9[0x200], 4);
        assert_eq!(arm9.len(), 0x250);
        assert_eq!(arm7, &[5; 0x80][..]);
    }

    #[test]
    fn a_packet_counts_once_however_often_it_comes() {
        let mut transfer = Transfer::new(0x200);
        transfer.take_rsa(&rsa(0x10, 0x10));
        assert!(transfer.take_packet(1, &[7; 0x10]));
        assert!(transfer.take_packet(1, &[7; 0x11]));
        assert_eq!(transfer.got(), 1);
        assert_eq!(transfer.next_missing(), 0);
    }

    #[test]
    fn a_frame_with_a_wrong_header_size_or_a_huge_binary_is_refused() {
        let mut transfer = Transfer::new(0x200);
        let mut bad = rsa(0x10, 0x10);
        bad[HEADER_SIZE_IN_RSA] = 0x70;
        assert!(!transfer.take_rsa(&bad));
        assert!(!transfer.take_rsa(&rsa(0x10, MAX_BINARY_BYTES as u32 + 1)));
        assert!(!transfer.take_rsa(&rsa(0x10, 0x10)[..0x40]));
        assert!(transfer.take_rsa(&rsa(0x10, 0x10)));
    }
}

use super::advert::Offer;
use crate::utils::crc16::crc16;

pub const HEADER_BYTES: usize = 0x160;
pub const RSA_BYTES: usize = 0xE4;
pub const MAX_ROM_BYTES: usize = 0x0400_0000;

const ARM9_SIZE_IN_RSA: usize = 0x24;
const ARM7_SIZE_IN_RSA: usize = 0x34;
const ARM9_BINARY: usize = 0x20;
const ARM7_BINARY: usize = 0x30;
const HEADER_CRC: usize = 0x15E;
const BSS_LENGTH: u16 = 0x0060;
const BSS_RSSI: u16 = 0x0082;
const SSID_BYTES: u16 = 0x20;
const CAPABILITY: u16 = 0x0021;
const RATES: u16 = 0x0003;
const DTIM_PERIOD: u16 = 0x0002;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Fault {
    HeaderCrc,
    Arm9Size,
    Arm7Size,
    Layout,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Parent {
    pub host: [u8; 6],
    pub channel: u8,
    pub game_id: u32,
    pub stream: u16,
    pub beacon_period: u16,
}

impl Parent {
    pub fn of(offer: &Offer) -> Self {
        Self {
            host: offer.host,
            channel: offer.channel,
            game_id: offer.game_id,
            stream: offer.stream,
            beacon_period: offer.beacon_period,
        }
    }

    pub fn boot_param(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(0x3C);
        bytes.extend_from_slice(&BSS_LENGTH.to_le_bytes());
        bytes.extend_from_slice(&BSS_RSSI.to_le_bytes());
        bytes.extend_from_slice(&self.host);
        bytes.extend_from_slice(&SSID_BYTES.to_le_bytes());
        let mut ssid = [0; SSID_BYTES as usize];
        ssid[..4].copy_from_slice(&self.game_id.to_le_bytes());
        ssid[4..6].copy_from_slice(&self.stream.to_le_bytes());
        bytes.extend_from_slice(&ssid);
        for value in [
            CAPABILITY,
            RATES,
            RATES,
            self.beacon_period,
            DTIM_PERIOD,
            u16::from(self.channel),
            0,
            0,
            0,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Image {
    pub header: Vec<u8>,
    pub arm9: Vec<u8>,
    pub arm7: Vec<u8>,
    pub parent: Parent,
}

impl Image {
    pub fn sizes_in_rsa(rsa: &[u8]) -> [usize; 2] {
        [ARM9_SIZE_IN_RSA, ARM7_SIZE_IN_RSA].map(|at| read32(rsa, at) as usize)
    }

    pub fn check(&self) -> Result<(), Fault> {
        if self.header.len() != HEADER_BYTES
            || crc16(0xFFFF, &self.header[..HEADER_CRC])
                != u16::from_le_bytes([self.header[HEADER_CRC], self.header[HEADER_CRC + 1]])
        {
            return Err(Fault::HeaderCrc);
        }
        let [arm9, arm7] = self.binaries();
        if arm9.size as usize != self.arm9.len() {
            return Err(Fault::Arm9Size);
        }
        if arm7.size as usize != self.arm7.len() {
            return Err(Fault::Arm7Size);
        }
        let [first, second] = if arm9.offset <= arm7.offset {
            [arm9, arm7]
        } else {
            [arm7, arm9]
        };
        let fits = first.offset as usize >= HEADER_BYTES
            && first.end() <= second.offset as usize
            && second.end() <= MAX_ROM_BYTES;
        if !fits {
            return Err(Fault::Layout);
        }
        Ok(())
    }

    pub fn rom(&self) -> Vec<u8> {
        let [arm9, arm7] = self.binaries();
        let mut rom = vec![0; arm9.end().max(arm7.end()).max(HEADER_BYTES)];
        rom[..HEADER_BYTES].copy_from_slice(&self.header);
        rom[arm9.offset as usize..arm9.end()].copy_from_slice(&self.arm9);
        rom[arm7.offset as usize..arm7.end()].copy_from_slice(&self.arm7);
        rom
    }

    fn binaries(&self) -> [Placed; 2] {
        [ARM9_BINARY, ARM7_BINARY].map(|at| Placed {
            offset: read32(&self.header, at),
            size: read32(&self.header, at + 12),
        })
    }
}

#[derive(Clone, Copy)]
struct Placed {
    offset: u32,
    size: u32,
}

impl Placed {
    fn end(self) -> usize {
        (self.offset as usize).saturating_add(self.size as usize)
    }
}

fn read32(bytes: &[u8], at: usize) -> u32 {
    bytes.get(at..at + 4).map_or(0, |word| {
        u32::from_le_bytes([word[0], word[1], word[2], word[3]])
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const ARM9_AT: u32 = 0x4000;

    pub(crate) fn image(arm9: usize, arm7: usize) -> Image {
        let arm7_at = (ARM9_AT as usize + arm9).next_multiple_of(0x200) as u32;
        let mut header = vec![0; HEADER_BYTES];
        header[..12].copy_from_slice(b"MARIOKARTDS\0");
        for (at, value) in [
            (ARM9_BINARY, ARM9_AT),
            (ARM9_BINARY + 4, 0x0200_0800),
            (ARM9_BINARY + 8, 0x0200_0000),
            (ARM9_BINARY + 12, arm9 as u32),
            (ARM7_BINARY, arm7_at),
            (ARM7_BINARY + 4, 0x0238_0000),
            (ARM7_BINARY + 8, 0x0238_0000),
            (ARM7_BINARY + 12, arm7 as u32),
        ] {
            header[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        seal(&mut header);
        Image {
            header,
            arm9: (0..arm9).map(|index| index as u8).collect(),
            arm7: (0..arm7).map(|index| !(index as u8)).collect(),
            parent: Parent {
                host: [0x00, 0x09, 0xBF, 0x12, 0x34, 0x56],
                channel: 7,
                game_id: 0x0040_00A0,
                stream: 0x1234,
                beacon_period: 0xC8,
            },
        }
    }

    pub(crate) fn with_code(arm9: &[u32], arm7: &[u32]) -> Image {
        let words =
            |code: &[u32]| -> Vec<u8> { code.iter().flat_map(|word| word.to_le_bytes()).collect() };
        let mut image = image(arm9.len() * 4, arm7.len() * 4);
        image.arm9 = words(arm9);
        image.arm7 = words(arm7);
        image.header[ARM9_BINARY + 4..ARM9_BINARY + 8]
            .copy_from_slice(&0x0200_0000u32.to_le_bytes());
        seal(&mut image.header);
        image
    }

    fn seal(header: &mut [u8]) {
        let crc = crc16(0xFFFF, &header[..HEADER_CRC]);
        header[HEADER_CRC..HEADER_CRC + 2].copy_from_slice(&crc.to_le_bytes());
    }

    #[test]
    fn a_whole_image_passes_and_lays_out_as_a_rom() {
        let image = image(0x1234, 0x200);
        assert_eq!(image.check(), Ok(()));
        let rom = image.rom();
        assert_eq!(&rom[..HEADER_BYTES], &image.header[..]);
        assert_eq!(&rom[0x4000..0x4000 + 0x1234], &image.arm9[..]);
        assert_eq!(&rom[0x5400..0x5600], &image.arm7[..]);
        assert_eq!(rom.len(), 0x5600);
    }

    #[test]
    fn a_damaged_header_or_short_binary_is_refused() {
        let mut bad = image(0x100, 0x100);
        bad.header[0] ^= 1;
        assert_eq!(bad.check(), Err(Fault::HeaderCrc));
        let mut short = image(0x100, 0x100);
        short.arm9.pop();
        assert_eq!(short.check(), Err(Fault::Arm9Size));
        let mut long = image(0x100, 0x100);
        long.arm7.push(0);
        assert_eq!(long.check(), Err(Fault::Arm7Size));
        let mut overlap = image(0x400, 0x100);
        overlap.header[ARM7_BINARY..ARM7_BINARY + 4].copy_from_slice(&0x4200u32.to_le_bytes());
        seal(&mut overlap.header);
        assert_eq!(overlap.check(), Err(Fault::Layout));
    }

    #[test]
    fn the_boot_block_matches_the_firmware_layout() {
        let block = image(0x10, 0x10).parent.boot_param();
        assert_eq!(block.len(), 0x3E);
        assert_eq!(&block[..4], &[0x60, 0x00, 0x82, 0x00]);
        assert_eq!(&block[0x04..0x0A], &[0x00, 0x09, 0xBF, 0x12, 0x34, 0x56]);
        assert_eq!(&block[0x0A..0x0C], &[0x20, 0x00]);
        assert_eq!(&block[0x0C..0x12], &[0xA0, 0x00, 0x40, 0x00, 0x34, 0x12]);
        assert_eq!(&block[0x2C..0x30], &[0x21, 0x00, 0x03, 0x00]);
        assert_eq!(&block[0x32..0x38], &[0xC8, 0x00, 0x02, 0x00, 0x07, 0x00]);
    }
}

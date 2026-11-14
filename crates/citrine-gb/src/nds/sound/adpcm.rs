pub const STEPS: [u16; 89] = [
    0x0007, 0x0008, 0x0009, 0x000A, 0x000B, 0x000C, 0x000D, 0x000E, 0x0010, 0x0011, 0x0013, 0x0015,
    0x0017, 0x0019, 0x001C, 0x001F, 0x0022, 0x0025, 0x0029, 0x002D, 0x0032, 0x0037, 0x003C, 0x0042,
    0x0049, 0x0050, 0x0058, 0x0061, 0x006B, 0x0076, 0x0082, 0x008F, 0x009D, 0x00AD, 0x00BE, 0x00D1,
    0x00E6, 0x00FD, 0x0117, 0x0133, 0x0151, 0x0173, 0x0198, 0x01C1, 0x01EE, 0x0220, 0x0256, 0x0292,
    0x02D4, 0x031C, 0x036C, 0x03C3, 0x0424, 0x048E, 0x0502, 0x0583, 0x0610, 0x06AB, 0x0756, 0x0812,
    0x08E0, 0x09C3, 0x0ABD, 0x0BD0, 0x0CFF, 0x0E4C, 0x0FBA, 0x114C, 0x1307, 0x14EE, 0x1706, 0x1954,
    0x1BDC, 0x1EA5, 0x21B6, 0x2515, 0x28CA, 0x2CDF, 0x315B, 0x364B, 0x3BB9, 0x41B2, 0x4844, 0x4F7E,
    0x5771, 0x602F, 0x69CE, 0x7462, 0x7FFF,
];
const INDEX_STEPS: [i8; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];
const MAX_INDEX: i8 = 88;
const MAX: i32 = 0x7FFF;
const SUBTRACT: u8 = 0x8;
const DIFFS: [[u16; 8]; STEPS.len()] = diffs();

const fn diffs() -> [[u16; 8]; STEPS.len()] {
    let mut table = [[0; 8]; STEPS.len()];
    let mut index = 0;
    while index < STEPS.len() {
        let step = STEPS[index];
        let mut nibble = 0;
        while nibble < 8 {
            let mut diff = step >> 3;
            if nibble & 1 != 0 {
                diff += step >> 2;
            }
            if nibble & 2 != 0 {
                diff += step >> 1;
            }
            if nibble & 4 != 0 {
                diff += step;
            }
            table[index][nibble] = diff;
            nibble += 1;
        }
        index += 1;
    }
    table
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Adpcm {
    pcm: i16,
    index: u8,
}

impl Adpcm {
    pub fn from_header(header: u32) -> Self {
        Self {
            pcm: header as u16 as i16,
            index: ((header >> 16) & 0x7F).min(MAX_INDEX as u32) as u8,
        }
    }

    pub fn pcm(self) -> i16 {
        self.pcm
    }

    pub fn decode(&mut self, nibble: u8) -> i16 {
        let diff = i32::from(DIFFS[usize::from(self.index)][usize::from(nibble & 7)]);
        let pcm = i32::from(self.pcm);
        let pcm = if nibble & SUBTRACT != 0 {
            (pcm - diff).max(-MAX)
        } else {
            (pcm + diff).min(MAX)
        };
        self.pcm = pcm as i16;
        let index = self.index as i8 + INDEX_STEPS[usize::from(nibble & 7)];
        self.index = index.clamp(0, MAX_INDEX) as u8;
        self.pcm
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decoder(pcm: i16, index: u8) -> Adpcm {
        Adpcm::from_header(u32::from(pcm as u16) | (u32::from(index) << 16))
    }

    #[test]
    fn the_step_table_matches_the_gbatek_generator() {
        let mut x: u32 = 0x0007_76D2;
        for (index, &step) in STEPS.iter().enumerate() {
            let expected = match index {
                3 => 0x000A,
                4 => 0x000B,
                88 => 0x7FFF,
                _ => (x >> 16) as u16,
            };
            assert_eq!(step, expected, "index {index}");
            x += x / 10;
        }
    }

    #[test]
    fn the_header_sets_pcm_and_index() {
        let adpcm = Adpcm::from_header(0x0023_8001);
        assert_eq!(adpcm.pcm(), -0x7FFF);
        assert_eq!(adpcm.index, 0x23);
    }

    #[test]
    fn a_header_index_past_88_is_clamped() {
        assert_eq!(Adpcm::from_header(0x007F_0000).index, 88);
    }

    #[test]
    fn a_known_sequence_decodes() {
        let mut adpcm = decoder(0, 0);
        let out: Vec<i16> = [0x0, 0x7, 0xF, 0x4, 0x9]
            .into_iter()
            .map(|nibble| adpcm.decode(nibble))
            .collect();
        assert_eq!(out, [0, 11, -19, 19, 4]);
        assert_eq!(adpcm.index, 17);
    }

    #[test]
    fn the_hardware_diff_rounds_each_part_down() {
        let mut adpcm = decoder(100, 0);
        assert_eq!(adpcm.decode(0x1), 101);
    }

    #[test]
    fn adding_clamps_at_plus_7fff() {
        let mut adpcm = decoder(0x7FF0, 88);
        assert_eq!(adpcm.decode(0x7), 0x7FFF);
        assert_eq!(adpcm.index, 88);
    }

    #[test]
    fn subtracting_clamps_at_minus_7fff() {
        let mut adpcm = decoder(-0x7FF0, 88);
        assert_eq!(adpcm.decode(0xF), -0x7FFF);
    }

    #[test]
    fn the_index_stays_at_zero_or_above() {
        let mut adpcm = decoder(0, 0);
        adpcm.decode(0x3);
        assert_eq!(adpcm.index, 0);
    }
}

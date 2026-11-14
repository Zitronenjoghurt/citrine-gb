const SPREAD: [u64; 256] = {
    let mut table = [0; 256];
    let mut byte = 0;
    while byte < 256 {
        let mut x = 0;
        while x < 8 {
            table[byte] |= ((byte as u64 >> (7 - x)) & 1) << (x * 8);
            x += 1;
        }
        byte += 1;
    }
    table
};

#[derive(Debug, Default, Copy, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TileLine {
    pub low: u8,
    pub high: u8,
}

impl TileLine {
    // x is the pixel column within the tile (0-7), 0 being the leftmost pixel
    // bit 7 of the low/high bytes corresponds to the leftmost pixel (x=0)
    // bit 0 corresponds to the rightmost pixel (x=7)
    pub fn color_index(&self, x: u8) -> u8 {
        let bit = 7 - (x & 7);
        let lo = (self.low >> bit) & 1;
        let hi = (self.high >> bit) & 1;
        // high bit is the MSB, low bit is the LSB of the 2-bit index
        (hi << 1) | lo
    }

    pub fn color_indices(&self) -> u64 {
        SPREAD[self.low as usize] | (SPREAD[self.high as usize] << 1)
    }
}

#[cfg(test)]
mod tests {
    use super::TileLine;

    #[test]
    fn color_indices_hold_each_pixel_in_its_byte() {
        for low in 0..=255 {
            for high in 0..=255 {
                let line = TileLine { low, high };
                let indices = line.color_indices();
                for x in 0..8u8 {
                    assert_eq!(
                        (indices >> (x * 8)) as u8,
                        line.color_index(x),
                        "{low} {high} {x}"
                    );
                }
            }
        }
    }
}

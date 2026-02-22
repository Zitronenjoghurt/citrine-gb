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
}

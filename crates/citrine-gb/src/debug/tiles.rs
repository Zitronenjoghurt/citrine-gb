#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Depth {
    Two,
    Four,
    Eight,
}

impl Depth {
    pub const fn tile_bytes(self) -> usize {
        match self {
            Depth::Two => 16,
            Depth::Four => 32,
            Depth::Eight => 64,
        }
    }

    pub const fn colors(self) -> usize {
        match self {
            Depth::Two => 4,
            Depth::Four => 16,
            Depth::Eight => 256,
        }
    }

    pub const fn bits(self) -> u8 {
        match self {
            Depth::Two => 2,
            Depth::Four => 4,
            Depth::Eight => 8,
        }
    }
}

pub type Pixels = [u8; 64];

pub fn decode(bytes: &[u8], depth: Depth) -> Pixels {
    let mut out = [0; 64];
    match depth {
        Depth::Two => {
            for (row, pair) in bytes[..16].as_chunks::<2>().0.iter().enumerate() {
                for col in 0..8 {
                    let bit = 7 - col;
                    let low = (pair[0] >> bit) & 1;
                    let high = (pair[1] >> bit) & 1;
                    out[row * 8 + col] = low | high << 1;
                }
            }
        }
        Depth::Four => {
            for (index, byte) in bytes[..32].iter().enumerate() {
                out[index * 2] = byte & 0x0F;
                out[index * 2 + 1] = byte >> 4;
            }
        }
        Depth::Eight => out.copy_from_slice(&bytes[..64]),
    }
    out
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Flip {
    pub x: bool,
    pub y: bool,
}

pub const CLEAR: [u8; 4] = [0, 0, 0, 0];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Picture {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

impl Picture {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            rgba: vec![0; width * height * 4],
        }
    }

    pub fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
        let at = (y * self.width + x) * 4;
        [
            self.rgba[at],
            self.rgba[at + 1],
            self.rgba[at + 2],
            self.rgba[at + 3],
        ]
    }

    pub fn set(&mut self, x: usize, y: usize, color: [u8; 4]) {
        if x < self.width && y < self.height {
            let at = (y * self.width + x) * 4;
            self.rgba[at..at + 4].copy_from_slice(&color);
        }
    }

    pub fn draw_tile(
        &mut self,
        x: usize,
        y: usize,
        pixels: &Pixels,
        colors: &[[u8; 4]],
        flip: Flip,
        clear_zero: bool,
    ) {
        for row in 0..8 {
            for col in 0..8 {
                let from_row = if flip.y { 7 - row } else { row };
                let from_col = if flip.x { 7 - col } else { col };
                let index = pixels[from_row * 8 + from_col];
                let color = if clear_zero && index == 0 {
                    CLEAR
                } else {
                    colors.get(usize::from(index)).copied().unwrap_or(CLEAR)
                };
                self.set(x + col, y + row, color);
            }
        }
    }

    pub fn png(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, self.width as u32, self.height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        if let Ok(mut writer) = encoder.write_header() {
            let _ = writer.write_image_data(&self.rgba);
        }
        out
    }
}

pub fn grey(depth: Depth) -> Vec<[u8; 4]> {
    let last = depth.colors() - 1;
    (0..=last)
        .map(|index| {
            let level = (255 - index * 255 / last) as u8;
            [level, level, level, 0xFF]
        })
        .collect()
}

pub fn sheet(bytes: &[u8], depth: Depth, columns: usize, colors: &[[u8; 4]]) -> Picture {
    let tiles = bytes.len() / depth.tile_bytes();
    let rows = tiles.div_ceil(columns);
    let mut picture = Picture::new(columns * 8, rows * 8);
    for (tile, data) in bytes.chunks_exact(depth.tile_bytes()).enumerate() {
        let pixels = decode(data, depth);
        let (x, y) = ((tile % columns) * 8, (tile / columns) * 8);
        picture.draw_tile(x, y, &pixels, colors, Flip::default(), false);
    }
    picture
}

pub fn wrapped(x: usize, y: usize, width: usize, height: usize, size: (usize, usize)) -> Vec<Rect> {
    let mut out = Vec::new();
    let (x, y) = (x % size.0, y % size.1);
    let first_width = width.min(size.0 - x);
    let first_height = height.min(size.1 - y);
    let widths = [(x, first_width), (0, width - first_width)];
    let heights = [(y, first_height), (0, height - first_height)];
    for &(top, h) in &heights {
        for &(left, w) in &widths {
            if w > 0 && h > 0 {
                out.push(Rect {
                    x: left,
                    y: top,
                    width: w,
                    height: h,
                });
            }
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_bits_read_low_then_high_plane_from_the_left() {
        let mut bytes = [0u8; 16];
        bytes[0] = 0b1000_0001;
        bytes[1] = 0b1100_0000;
        let pixels = decode(&bytes, Depth::Two);
        assert_eq!(&pixels[..8], &[3, 2, 0, 0, 0, 0, 0, 1]);
    }

    #[test]
    fn four_bits_read_the_low_nibble_first() {
        let mut bytes = [0u8; 32];
        bytes[0] = 0x21;
        bytes[31] = 0xF0;
        let pixels = decode(&bytes, Depth::Four);
        assert_eq!(&pixels[..2], &[1, 2]);
        assert_eq!(&pixels[62..], &[0, 15]);
    }

    #[test]
    fn eight_bits_are_one_byte_per_pixel() {
        let bytes: Vec<u8> = (0..64).collect();
        assert_eq!(decode(&bytes, Depth::Eight).to_vec(), bytes);
    }

    #[test]
    fn a_flipped_tile_draws_mirrored() {
        let mut pixels = [0u8; 64];
        pixels[0] = 1;
        let colors = [[0, 0, 0, 255], [255, 0, 0, 255]];
        let mut picture = Picture::new(8, 8);
        picture.draw_tile(0, 0, &pixels, &colors, Flip { x: true, y: true }, false);
        assert_eq!(picture.pixel(7, 7), [255, 0, 0, 255]);
        assert_eq!(picture.pixel(0, 0), [0, 0, 0, 255]);
    }

    #[test]
    fn a_frame_past_the_edge_wraps_into_four_pieces() {
        let pieces = wrapped(200, 150, 160, 144, (256, 256));
        assert_eq!(pieces.len(), 4);
        let area: usize = pieces.iter().map(|rect| rect.width * rect.height).sum();
        assert_eq!(area, 160 * 144);
        assert_eq!(wrapped(0, 0, 160, 144, (256, 256)).len(), 1);
    }
}

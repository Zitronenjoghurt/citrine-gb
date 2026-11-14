use super::{LcdcRead, WIDTH};

pub(super) const BANK_BYTES: u32 = 0x2_0000;
const LINE_BYTES: u32 = WIDTH as u32 * 2;

#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(super) struct Fifo {
    pixels: Vec<u16>,
}

impl Fifo {
    pub fn push(&mut self, pixel: u16) {
        if self.pixels.len() < WIDTH {
            self.pixels.push(pixel);
        }
    }

    pub fn room(&self) -> usize {
        WIDTH - self.pixels.len()
    }

    pub fn take_line(&mut self) -> [u16; WIDTH] {
        let mut line = [0; WIDTH];
        line[..self.pixels.len()].copy_from_slice(&self.pixels);
        self.pixels.clear();
        line
    }
}

pub(super) fn vram_line<L: LcdcRead>(lcdc: &L, block: u32, offset: u32, y: usize) -> [u16; WIDTH] {
    let start = offset + y as u32 * LINE_BYTES;
    let mut line = [0; WIDTH];
    for (x, pixel) in line.iter_mut().enumerate() {
        let at = (start + x as u32 * 2) % BANK_BYTES;
        *pixel = lcdc.lcdc_u16(block * BANK_BYTES + at);
    }
    line
}

#[cfg(test)]
mod tests {
    use super::super::tests::{Banks, Scene};
    use super::super::{EngineKind, HEIGHT, WIDTH};

    const DISPCNT: usize = 0x00;
    const FIFO_LOW: usize = 0x68;
    const FIFO_HIGH: usize = 0x6A;
    const MASTER_BRIGHT: usize = 0x6C;
    const GRAPHICS: u32 = 1 << 16;
    const VRAM: u32 = 2 << 16;
    const FIFO: u32 = 3 << 16;
    const DOWN: u16 = 2 << 14;
    const WHITE: u16 = 0x7FFF;

    fn show(scene: &mut Scene, y: usize, banks: &mut Banks) -> [u16; WIDTH] {
        scene.engine.start_line(HEIGHT as u16);
        scene.engine.start_line(y as u16);
        scene.display(y, banks)
    }

    #[test]
    fn display_off_is_white_and_master_brightness_leaves_it() {
        let mut scene = Scene::new(EngineKind::A);
        scene.color(0, 0x1234);
        scene.write(MASTER_BRIGHT, DOWN | 16);
        assert_eq!(show(&mut scene, 0, &mut Banks::new()), [WHITE; WIDTH]);
    }

    #[test]
    fn graphics_display_shows_the_engine_line_through_master_brightness() {
        let mut scene = Scene::new(EngineKind::A);
        scene.write32(DISPCNT, GRAPHICS);
        scene.color(0, 0x1234);
        let mut banks = Banks::new();
        assert_eq!(show(&mut scene, 0, &mut banks), [0x1234; WIDTH]);
        scene.write(MASTER_BRIGHT, DOWN | 16);
        assert_eq!(show(&mut scene, 0, &mut banks), [0; WIDTH]);
    }

    #[test]
    fn vram_display_shows_the_dispcnt_block_without_bit_15_through_master_brightness() {
        let mut scene = Scene::new(EngineKind::A);
        scene.write32(DISPCNT, VRAM | 2 << 18);
        let mut banks = Banks::new();
        banks.put(0x4_0000 + 3 * 512 + 2, 0xFFFF);
        banks.put(0x4_0000 + 3 * 512 + 4, 0x1234);
        banks.put(0x2_0000 + 3 * 512 + 2, 0x0042);
        let out = show(&mut scene, 3, &mut banks);
        assert_eq!(out[..4], [0, WHITE, 0x1234, 0]);
        scene.write(MASTER_BRIGHT, DOWN | 8);
        assert_eq!(show(&mut scene, 3, &mut banks)[1], 15 | 15 << 5 | 15 << 10);
    }

    #[test]
    fn fifo_display_shows_pushed_pixels_in_order_and_pads_with_black() {
        let mut scene = Scene::new(EngineKind::A);
        scene.write32(DISPCNT, FIFO);
        let mut banks = Banks::new();
        scene.write(FIFO_LOW, 0x8001);
        scene.write(FIFO_HIGH, 0x0002);
        scene.write(FIFO_LOW, 0x0003);
        assert_eq!(scene.engine.fifo_room(), WIDTH - 3);
        let out = show(&mut scene, 0, &mut banks);
        assert_eq!(out[..4], [1, 2, 3, 0]);
        assert_eq!(scene.engine.fifo_room(), WIDTH);
        for i in 0..WIDTH + 4 {
            scene.write(FIFO_LOW, i as u16);
        }
        assert_eq!(scene.engine.fifo_room(), 0);
        let out = show(&mut scene, 1, &mut banks);
        assert_eq!(out[WIDTH - 1], WIDTH as u16 - 1);
        assert_eq!(show(&mut scene, 2, &mut banks), [0; WIDTH]);
    }

    #[test]
    fn engine_b_has_no_fifo() {
        let mut scene = Scene::new(EngineKind::B);
        scene.write(FIFO_LOW, 1);
        assert_eq!(scene.engine.fifo_room(), WIDTH);
    }
}

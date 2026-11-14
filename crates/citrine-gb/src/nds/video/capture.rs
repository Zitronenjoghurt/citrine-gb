use super::display::BANK_BYTES;
use super::{HEIGHT, LcdcWrite, WIDTH};

const OFFSET_STEP: u32 = 0x8000;
const ENABLE: u32 = 1 << 31;
const SOURCE_A_3D: u32 = 1 << 24;
const SOURCE_B_FIFO: u32 = 1 << 25;
const MAX_FACTOR: u16 = 16;
const MAX_CHANNEL: u16 = 31;
const ROUND: u16 = 8;
const ALPHA: u16 = 1 << 15;
const WRITABLE: u32 = 0xEF3F_1F1F;
const SIZES: [(usize, usize); 4] = [(128, 128), (256, 64), (256, 128), (256, 192)];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SourceA {
    Graphics,
    ThreeD,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SourceB {
    Vram { offset: u32 },
    Fifo,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Request {
    pub a: Option<SourceA>,
    pub b: Option<SourceB>,
    width: usize,
    y: usize,
    control: u32,
}

#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(super) struct Capture {
    control: u32,
    active: bool,
}

impl Capture {
    pub fn control(&self) -> u32 {
        self.control
    }

    pub fn write(&mut self, shift: u32, value: u16, mask: u16) {
        super::merge32(&mut self.control, shift, value, mask);
        self.control &= WRITABLE;
    }

    pub fn start_line(&mut self, line: usize) {
        if line == 0 {
            self.active = self.control & ENABLE != 0;
        } else if line == HEIGHT && self.active {
            self.control &= !ENABLE;
            self.active = false;
        }
    }

    pub fn request(&self, y: usize) -> Option<Request> {
        let (width, height) = SIZES[(self.control >> 20 & 0b11) as usize];
        if !self.active || self.control & ENABLE == 0 || y >= height {
            return None;
        }
        let source = self.control >> 29 & 0b11;
        let a = (source != 1).then_some(if self.control & SOURCE_A_3D != 0 {
            SourceA::ThreeD
        } else {
            SourceA::Graphics
        });
        let b = (source != 0).then_some(if self.control & SOURCE_B_FIFO != 0 {
            SourceB::Fifo
        } else {
            SourceB::Vram {
                offset: (self.control >> 26 & 0b11) * OFFSET_STEP,
            }
        });
        Some(Request {
            a,
            b,
            width,
            y,
            control: self.control,
        })
    }
}

impl Request {
    pub fn line(&self, a: &[u16; WIDTH], b: &[u16; WIDTH]) -> CaptureLine {
        let eva = (self.control as u16 & 0x1F).min(MAX_FACTOR);
        let evb = (self.control >> 8 & 0x1F).min(u32::from(MAX_FACTOR)) as u16;
        let mut pixels = [0; WIDTH];
        for (x, pixel) in pixels.iter_mut().enumerate().take(self.width) {
            *pixel = match (self.a, self.b) {
                (Some(_), None) => a[x],
                (None, _) => b[x],
                (Some(_), Some(_)) => blend(a[x], b[x], eva, evb),
            };
        }
        let bank = self.control >> 16 & 0b11;
        let offset = (self.control >> 18 & 0b11) * OFFSET_STEP;
        CaptureLine {
            bank: bank * BANK_BYTES,
            offset: offset + (self.y * self.width * 2) as u32,
            pixels,
            width: self.width,
        }
    }
}

fn blend(a: u16, b: u16, eva: u16, evb: u16) -> u16 {
    let alpha_a = a >> 15;
    let alpha_b = b >> 15;
    let color = [0, 5, 10].into_iter().fold(0, |out, shift| {
        let channel_a = a >> shift & 0x1F;
        let channel_b = b >> shift & 0x1F;
        let mixed = (channel_a * alpha_a * eva + channel_b * alpha_b * evb + ROUND) >> 4;
        out | mixed.min(MAX_CHANNEL) << shift
    });
    let alpha = (alpha_a != 0 && eva > 0) || (alpha_b != 0 && evb > 0);
    if alpha { color | ALPHA } else { color }
}

#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureLine {
    bank: u32,
    offset: u32,
    pixels: [u16; WIDTH],
    width: usize,
}

impl CaptureLine {
    pub fn write<T: LcdcWrite>(&self, target: &mut T) {
        for (x, &pixel) in self.pixels[..self.width].iter().enumerate() {
            let offset = (self.offset + x as u32 * 2) % BANK_BYTES;
            target.write_lcdc_u16(self.bank + offset, pixel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{Banks, Scene};
    use super::super::{EngineKind, HEIGHT, Line3d, WIDTH, color};

    const DISPCNT: usize = 0x00;
    const BG0HOFS: usize = 0x10;
    const CAPTURE: usize = 0x64;
    const FIFO_LOW: usize = 0x68;
    const MASTER_BRIGHT: usize = 0x6C;
    const GRAPHICS: u32 = 1 << 16;
    const VRAM: u32 = 2 << 16;
    const ENABLE: u32 = 1 << 31;
    const FULL: u32 = 3 << 20;
    const THREE_D: u32 = 1 << 24;
    const FIFO: u32 = 1 << 25;
    const ONLY_B: u32 = 1 << 29;
    const BLENDED: u32 = 2 << 29;
    const BANK_B: u32 = 0x2_0000;
    const RED: u16 = 0x001F;
    const BLUE: u16 = 0x7C00;
    const ALPHA: u16 = 0x8000;

    fn ready(control: u32) -> (Scene, Banks) {
        let mut scene = Scene::new(EngineKind::A);
        scene.write32(DISPCNT, GRAPHICS);
        scene.write32(CAPTURE, ENABLE | control);
        scene.engine.start_line(HEIGHT as u16);
        (scene, Banks::new())
    }

    fn frame_line(scene: &mut Scene, banks: &mut Banks, y: usize) -> [u16; WIDTH] {
        scene.engine.start_line(y as u16);
        scene.display(y, banks)
    }

    #[test]
    fn capture_starts_at_line_0_and_clears_its_enable_bit_at_line_192() {
        let (mut scene, mut banks) = ready(FULL | 1 << 16);
        scene.color(0, RED);
        scene.engine.start_line(5);
        scene.display(5, &mut banks);
        assert_eq!(banks.at(BANK_B + 5 * 512), 0);
        for y in 0..HEIGHT {
            frame_line(&mut scene, &mut banks, y);
        }
        assert_eq!(banks.at(BANK_B + 191 * 512 + 510), RED | ALPHA);
        assert_eq!(scene.engine.read(CAPTURE + 2) >> 15, 1);
        scene.engine.start_line(HEIGHT as u16);
        assert_eq!(scene.engine.read(CAPTURE + 2) >> 15, 0);
        scene.color(0, BLUE);
        frame_line(&mut scene, &mut banks, 0);
        assert_eq!(banks.at(BANK_B), RED | ALPHA);
    }

    #[test]
    fn source_a_takes_the_graphics_line_before_master_brightness() {
        let (mut scene, mut banks) = ready(FULL | 1 << 16 | 1 << 18);
        scene.color(0, 0x1234);
        scene.write(MASTER_BRIGHT, 2 << 14 | 16);
        frame_line(&mut scene, &mut banks, 0);
        let out = frame_line(&mut scene, &mut banks, 2);
        assert_eq!(out[0], 0);
        assert_eq!(banks.at(BANK_B + 0x8000 + 2 * 512), 0x1234 | ALPHA);
    }

    #[test]
    fn source_a_3d_takes_the_raw_3d_line_with_its_alpha() {
        let (mut scene, mut banks) = ready(FULL | THREE_D);
        scene.write32(DISPCNT, 0);
        scene.write(BG0HOFS, 5);
        let mut line = Line3d::default();
        line.color[0] = color::widen(0x03E0) | 1;
        line.alpha[0] = 1;
        line.color[1] = color::widen(RED);
        scene.three_d = Some(line);
        frame_line(&mut scene, &mut banks, 0);
        assert_eq!([banks.at(0), banks.at(2)], [0x03E0 | ALPHA, RED]);
        scene.three_d = None;
        frame_line(&mut scene, &mut banks, 1);
        assert_eq!(banks.at(512), 0);
    }

    #[test]
    fn source_b_reads_the_dispcnt_block_at_the_read_offset_unless_vram_display_is_on() {
        let (mut scene, mut banks) = ready(FULL | ONLY_B | 2 << 26 | 1 << 16);
        scene.write32(DISPCNT, GRAPHICS | 3 << 18);
        banks.put(0x6_0000 + 0x1_0000 + 4 * 512, 0x8123);
        banks.put(0x6_0000 + 4 * 512, 0x0456);
        frame_line(&mut scene, &mut banks, 0);
        frame_line(&mut scene, &mut banks, 4);
        assert_eq!(banks.at(BANK_B + 4 * 512), 0x8123);
        scene.write32(DISPCNT, VRAM | 3 << 18);
        let out = frame_line(&mut scene, &mut banks, 4);
        assert_eq!(out[0], 0x0456);
        assert_eq!(banks.at(BANK_B + 4 * 512), 0x0456);
    }

    #[test]
    fn source_b_can_take_the_fifo_line() {
        let (mut scene, mut banks) = ready(FULL | ONLY_B | FIFO);
        scene.write(FIFO_LOW, 0x8007);
        scene.write(FIFO_LOW, 0x0008);
        frame_line(&mut scene, &mut banks, 0);
        assert_eq!([banks.at(0), banks.at(2), banks.at(4)], [0x8007, 0x0008, 0]);
        assert_eq!(scene.engine.fifo_room(), WIDTH);
    }

    #[test]
    fn blending_weighs_each_source_by_its_alpha_bit_and_factor() {
        let (mut scene, mut banks) = ready(FULL | BLENDED | 8 | 8 << 8);
        scene.color(0, RED);
        banks.put(0, BLUE | ALPHA);
        banks.put(2, BLUE);
        frame_line(&mut scene, &mut banks, 0);
        assert_eq!(banks.at(0), 16 | 16 << 10 | ALPHA);
        assert_eq!(banks.at(2), 16 | ALPHA);
        scene.write32(CAPTURE, ENABLE | FULL | BLENDED | 8 << 8);
        banks.put(0, BLUE | ALPHA);
        banks.put(2, BLUE);
        frame_line(&mut scene, &mut banks, 0);
        assert_eq!([banks.at(0), banks.at(2)], [16 << 10 | ALPHA, 0]);
        scene.write32(CAPTURE, ENABLE | FULL | BLENDED | 31 | 16 << 8);
        banks.put(0, RED | ALPHA);
        frame_line(&mut scene, &mut banks, 0);
        assert_eq!(banks.at(0), RED | ALPHA);
    }

    #[test]
    fn a_128_dot_capture_writes_128_pixels_per_line_for_128_lines() {
        let (mut scene, mut banks) = ready(1 << 16);
        scene.color(0, RED);
        frame_line(&mut scene, &mut banks, 0);
        assert_eq!(
            [banks.at(BANK_B + 254), banks.at(BANK_B + 256)],
            [RED | ALPHA, 0]
        );
        frame_line(&mut scene, &mut banks, 127);
        assert_eq!(banks.at(BANK_B + 127 * 256), RED | ALPHA);
        frame_line(&mut scene, &mut banks, 128);
        assert_eq!(banks.at(BANK_B + 128 * 256), 0);
    }

    #[test]
    fn the_write_offset_wraps_inside_the_bank() {
        let (mut scene, mut banks) = ready(FULL | 1 << 16 | 3 << 18);
        scene.color(0, RED);
        frame_line(&mut scene, &mut banks, 0);
        frame_line(&mut scene, &mut banks, 64);
        assert_eq!(banks.at(BANK_B), RED | ALPHA);
        assert_eq!(banks.at(BANK_B * 2), 0);
    }

    #[test]
    fn engine_b_has_no_capture() {
        let mut scene = Scene::new(EngineKind::B);
        scene.write32(DISPCNT, GRAPHICS);
        scene.write32(CAPTURE, ENABLE | FULL);
        assert_eq!(scene.engine.read(CAPTURE + 2), 0);
        let mut banks = Banks::new();
        scene.color(0, RED);
        scene.engine.start_line(0);
        scene.display(0, &mut banks);
        assert_eq!(banks.at(0), 0);
    }
}

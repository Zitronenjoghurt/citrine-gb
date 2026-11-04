use crate::gb::cartridge::RAM_BANK_SIZE;
use crate::gb::cartridge::mbc::{MbcInterface, mask_bank_number};

pub const CAMERA_WIDTH: usize = 128;
pub const CAMERA_HEIGHT: usize = 112;
pub const CAMERA_PIXELS: usize = CAMERA_WIDTH * CAMERA_HEIGHT;
const REGISTERS: usize = 0x36;
const IMAGE_OFFSET: usize = 0x100;
const ALPHA: [f32; 8] = [0.5, 0.75, 1.0, 1.25, 2.0, 3.0, 4.0, 5.0];

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Camera {
    pub rom_bank_count: usize,
    pub ram_bank_count: usize,
    pub ram_enabled: bool,
    pub rom_bank_register: u8,
    pub ram_select: u8,
    pub registers: Vec<u8>,
    pub cycles_left: u32,
    #[cfg_attr(feature = "serde", serde(skip, default))]
    pub frame: Option<Vec<u8>>,
}

impl Camera {
    pub fn new(rom_bank_count: usize, ram_bank_count: usize) -> Self {
        Self {
            rom_bank_count,
            ram_bank_count,
            ram_enabled: false,
            rom_bank_register: 1,
            ram_select: 0,
            registers: vec![0; REGISTERS],
            cycles_left: 0,
            frame: None,
        }
    }

    pub fn set_frame(&mut self, frame: &[u8]) {
        if frame.len() == CAMERA_PIXELS {
            self.frame = Some(frame.to_vec());
        }
    }

    pub fn busy(&self) -> bool {
        self.cycles_left > 0
    }

    fn registers_selected(&self) -> bool {
        self.ram_select & 0x10 != 0
    }

    fn exposure(&self) -> u32 {
        u32::from(self.registers[3]) | (u32::from(self.registers[2]) << 8)
    }

    fn capture_cycles(&self) -> u32 {
        let n = self.registers[1] & 0x80 != 0;
        32446 + if n { 0 } else { 512 } + 16 * self.exposure()
    }

    pub fn cycle(&mut self, ram: &mut [[u8; RAM_BANK_SIZE]]) -> bool {
        if self.cycles_left == 0 {
            return false;
        }
        self.cycles_left -= 1;
        if self.cycles_left > 0 {
            return false;
        }
        self.registers[0] &= !0x01;
        let tiles = self.capture();
        if let Some(bank) = ram.first_mut() {
            bank[IMAGE_OFFSET..IMAGE_OFFSET + tiles.len()].copy_from_slice(&tiles);
        }
        true
    }

    fn write_register(&mut self, index: usize, value: u8) {
        if index >= REGISTERS {
            return;
        }
        if index == 0 {
            let value = value & 0x07;
            if value & 0x01 != 0 && self.cycles_left == 0 {
                self.cycles_left = self.capture_cycles();
            } else if value & 0x01 == 0 {
                self.cycles_left = 0;
            }
            self.registers[0] = value;
        } else {
            self.registers[index] = value;
        }
    }

    pub fn capture(&self) -> Vec<u8> {
        let input = match &self.frame {
            Some(frame) => frame.clone(),
            None => test_frame(),
        };
        let sensed = self.sense(&input);
        self.dither(&sensed)
    }

    fn sense(&self, input: &[u8]) -> Vec<i32> {
        let exposure = self.exposure() as i32;
        let invert = self.registers[4] & 0x08 != 0;
        let signed: Vec<i32> = input
            .iter()
            .map(|&pixel| {
                let scaled = pixel as i32 * exposure / 0x0300;
                let value = (128 + (scaled - 128) / 8).clamp(0, 255);
                let value = if invert { 255 - value } else { value };
                value - 128
            })
            .collect();
        let at = |x: i32, y: i32| {
            let x = x.clamp(0, CAMERA_WIDTH as i32 - 1) as usize;
            let y = y.clamp(0, CAMERA_HEIGHT as i32 - 1) as usize;
            signed[y * CAMERA_WIDTH + x]
        };
        let n = (self.registers[1] >> 7) & 0x01;
        let vh = (self.registers[1] >> 5) & 0x03;
        let e3 = (self.registers[4] >> 7) & 0x01;
        let alpha = ALPHA[((self.registers[4] >> 4) & 0x07) as usize];
        let (p, m) = match (self.registers[0] >> 1) & 0x03 {
            0 => (0, 1),
            1 => (1, 0),
            _ => (1, 2),
        };
        let one_d = |value: i32, south: i32| {
            let mut out = 0;
            if p & 0x01 != 0 {
                out += value;
            }
            if p & 0x02 != 0 {
                out += south;
            }
            if m & 0x01 != 0 {
                out -= value;
            }
            if m & 0x02 != 0 {
                out -= south;
            }
            out
        };
        let mut out = vec![0i32; CAMERA_PIXELS];
        for y in 0..CAMERA_HEIGHT as i32 {
            for x in 0..CAMERA_WIDTH as i32 {
                let px = at(x, y);
                let value = match (n << 3) | (vh << 1) | e3 {
                    0x0 => one_d(px, at(x, y + 1)),
                    0x1 => -128,
                    0x2 => {
                        let edge = |cx: i32, cy: i32| {
                            let centre = at(cx, cy);
                            let sum = 2 * centre - at(cx - 1, cy) - at(cx + 1, cy);
                            centre + (sum as f32 * alpha) as i32
                        };
                        one_d(edge(x, y), edge(x, y + 1))
                    }
                    0xE => {
                        let sum =
                            4 * px - at(x, y - 1) - at(x, y + 1) - at(x - 1, y) - at(x + 1, y);
                        px + (sum as f32 * alpha) as i32
                    }
                    _ => px,
                };
                out[(y as usize) * CAMERA_WIDTH + x as usize] = (value + 128).clamp(0, 255);
            }
        }
        out
    }

    fn dither(&self, sensed: &[i32]) -> Vec<u8> {
        let mut tiles = vec![0u8; CAMERA_PIXELS / 4];
        for y in 0..CAMERA_HEIGHT {
            for x in 0..CAMERA_WIDTH {
                let cell = 6 + ((y & 3) * 4 + (x & 3)) * 3;
                let thresholds = &self.registers[cell..cell + 3];
                let value = sensed[y * CAMERA_WIDTH + x];
                let level = thresholds
                    .iter()
                    .take_while(|&&threshold| value >= threshold as i32)
                    .count() as u8;
                let colour = 3 - level;
                let tile = (y / 8) * (CAMERA_WIDTH / 8) + x / 8;
                let row = tile * 16 + (y % 8) * 2;
                let bit = 7 - (x % 8);
                tiles[row] |= (colour & 0x01) << bit;
                tiles[row + 1] |= ((colour >> 1) & 0x01) << bit;
            }
        }
        tiles
    }
}

pub fn test_frame() -> Vec<u8> {
    let mut frame = vec![0u8; CAMERA_PIXELS];
    for y in 0..CAMERA_HEIGHT {
        for x in 0..CAMERA_WIDTH {
            let gradient = (x * 255 / (CAMERA_WIDTH - 1)) as u8;
            let in_box = (40..88).contains(&x) && (32..80).contains(&y);
            let checker = ((x / 8) + (y / 8)) % 2 == 0;
            frame[y * CAMERA_WIDTH + x] = match (in_box, checker) {
                (true, true) => 255,
                (true, false) => 0,
                _ => gradient,
            };
        }
    }
    frame
}

impl MbcInterface for Camera {
    fn on_write(&mut self, addr: u16, value: u8) -> bool {
        match addr {
            0x0000..=0x1FFF => self.ram_enabled = value & 0x0F == 0x0A,
            0x2000..=0x3FFF => self.rom_bank_register = value & 0x3F,
            0x4000..=0x5FFF => self.ram_select = value,
            0xA000..=0xBFFF if self.registers_selected() => {
                self.write_register((addr as usize - 0xA000) & 0x7F, value);
            }
            _ => {}
        }
        false
    }

    fn on_read(&self, addr: u16) -> Option<u8> {
        if !(0xA000..=0xBFFF).contains(&addr) {
            return None;
        }
        if self.registers_selected() {
            return Some(match (addr as usize - 0xA000) & 0x7F {
                0 => self.registers[0] & 0x07,
                _ => 0x00,
            });
        }
        if self.busy() { Some(0x00) } else { None }
    }

    fn rom_bank_low(&self) -> usize {
        0
    }

    fn rom_bank_high(&self) -> usize {
        mask_bank_number(self.rom_bank_register as usize, self.rom_bank_count)
    }

    fn ram_bank(&self) -> Option<usize> {
        if self.registers_selected() {
            None
        } else {
            Some(mask_bank_number(
                (self.ram_select & 0x0F) as usize,
                self.ram_bank_count.max(1),
            ))
        }
    }

    fn ram_write_offset(&self, addr: u16) -> Option<usize> {
        if self.ram_enabled && !self.busy() {
            self.ram_offset(addr)
        } else {
            None
        }
    }

    fn soft_reset(&mut self) {
        let frame = self.frame.take();
        *self = Self::new(self.rom_bank_count, self.ram_bank_count);
        self.frame = frame;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera() -> Camera {
        let mut camera = Camera::new(64, 16);
        camera.on_write(0x0000, 0x0A);
        camera.on_write(0x4000, 0x10);
        camera
    }

    fn set_matrix(camera: &mut Camera, thresholds: [u8; 3]) {
        for cell in 0..16 {
            for (i, threshold) in thresholds.iter().enumerate() {
                camera.on_write(0xA006 + cell * 3 + i as u16, *threshold);
            }
        }
    }

    fn pixel(tiles: &[u8], x: usize, y: usize) -> u8 {
        let tile = (y / 8) * 16 + x / 8;
        let row = tile * 16 + (y % 8) * 2;
        let bit = 7 - (x % 8);
        ((tiles[row] >> bit) & 1) | (((tiles[row + 1] >> bit) & 1) << 1)
    }

    #[test]
    fn rom_bank_zero_is_allowed_and_bank_one_starts_mapped() {
        let mut camera = Camera::new(64, 16);
        assert_eq!(camera.rom_bank_high(), 1);
        camera.on_write(0x2000, 0x00);
        assert_eq!(camera.rom_bank_high(), 0);
        camera.on_write(0x2000, 0xFF);
        assert_eq!(camera.rom_bank_high(), 0x3F);
    }

    #[test]
    fn ram_reads_need_no_enable_but_writes_do() {
        let mut camera = Camera::new(64, 16);
        camera.on_write(0x4000, 0x05);
        assert_eq!(camera.ram_offset(0xA001), Some(5 * 0x2000 + 1));
        assert_eq!(camera.ram_write_offset(0xA001), None);
        camera.on_write(0x0000, 0x0A);
        assert_eq!(camera.ram_write_offset(0xA001), Some(5 * 0x2000 + 1));
    }

    #[test]
    fn registers_mirror_every_0x80_bytes_and_only_a000_reads_back() {
        let mut camera = camera();
        camera.on_write(0xA082, 0x12);
        assert_eq!(camera.registers[2], 0x12);
        assert_eq!(camera.on_read(0xA002), Some(0x00));
        camera.on_write(0xA000, 0xFE);
        assert_eq!(camera.on_read(0xA080), Some(0x06));
    }

    #[test]
    fn capture_takes_the_documented_time() {
        let mut camera = camera();
        camera.on_write(0xA001, 0x80);
        camera.on_write(0xA002, 0x01);
        camera.on_write(0xA003, 0x00);
        camera.on_write(0xA000, 0x01);
        assert_eq!(camera.cycles_left, 32446 + 16 * 0x100);
        camera.on_write(0xA001, 0x00);
        camera.on_write(0xA000, 0x00);
        camera.on_write(0xA000, 0x01);
        assert_eq!(camera.cycles_left, 32446 + 512 + 16 * 0x100);
    }

    #[test]
    fn ram_reads_zero_and_ignores_writes_while_capturing() {
        let mut camera = camera();
        camera.on_write(0xA000, 0x01);
        camera.on_write(0x4000, 0x00);
        assert_eq!(camera.on_read(0xA100), Some(0x00));
        assert_eq!(camera.ram_write_offset(0xA100), None);
        camera.on_write(0x4000, 0x10);
        assert_eq!(camera.on_read(0xA000), Some(0x01));
    }

    #[test]
    fn a_finished_capture_writes_tiles_to_bank_zero_and_clears_busy() {
        let mut camera = camera();
        camera.on_write(0xA002, 0x03);
        set_matrix(&mut camera, [0x72, 0x78, 0x80]);
        camera.set_frame(&vec![255; CAMERA_PIXELS]);
        camera.on_write(0xA000, 0x03);
        let mut ram = vec![[0u8; RAM_BANK_SIZE]; 16];
        let mut wrote = false;
        while camera.busy() {
            wrote = camera.cycle(&mut ram);
        }
        assert!(wrote);
        assert_eq!(camera.on_read(0xA000), Some(0x02));
        let tiles = &ram[0][IMAGE_OFFSET..IMAGE_OFFSET + 0xE00];
        assert_eq!(pixel(tiles, 0, 0), 0);
        assert_eq!(pixel(tiles, 127, 111), 0);
        assert_eq!(ram[0][IMAGE_OFFSET + 0xE00], 0);
    }

    #[test]
    fn dark_input_gives_black_and_invert_flips_it() {
        let mut camera = camera();
        camera.on_write(0xA002, 0x03);
        camera.on_write(0xA000, 0x02);
        set_matrix(&mut camera, [0x72, 0x78, 0x80]);
        camera.set_frame(&vec![0; CAMERA_PIXELS]);
        assert_eq!(pixel(&camera.capture(), 10, 10), 3);
        camera.on_write(0xA004, 0x08);
        assert_eq!(pixel(&camera.capture(), 10, 10), 0);
    }

    #[test]
    fn matrix_thresholds_pick_the_grey_level() {
        let mut camera = camera();
        camera.on_write(0xA002, 0x03);
        camera.on_write(0xA000, 0x02);
        set_matrix(&mut camera, [0x00, 0x80, 0xFF]);
        camera.set_frame(&vec![128; CAMERA_PIXELS]);
        assert_eq!(pixel(&camera.capture(), 5, 5), 1);
    }

    #[test]
    fn mode_0x1_outputs_black() {
        let mut camera = camera();
        camera.on_write(0xA002, 0x03);
        camera.on_write(0xA004, 0x80);
        set_matrix(&mut camera, [0x10, 0x20, 0x30]);
        camera.set_frame(&vec![255; CAMERA_PIXELS]);
        assert_eq!(pixel(&camera.capture(), 0, 0), 3);
    }

    #[test]
    fn without_a_frame_the_capture_shows_the_test_pattern() {
        let mut camera = camera();
        camera.on_write(0xA002, 0x03);
        camera.on_write(0xA000, 0x02);
        set_matrix(&mut camera, [0x78, 0x80, 0x88]);
        let tiles = camera.capture();
        assert_ne!(pixel(&tiles, 0, 0), pixel(&tiles, 127, 0));
        assert_ne!(pixel(&tiles, 40, 32), pixel(&tiles, 48, 32));
    }

    #[test]
    fn soft_reset_keeps_the_frame() {
        let mut camera = camera();
        camera.set_frame(&vec![7; CAMERA_PIXELS]);
        camera.soft_reset();
        assert_eq!(camera.frame.as_deref().map(|f| f[0]), Some(7));
        assert_eq!(camera.registers[0], 0);
    }
}

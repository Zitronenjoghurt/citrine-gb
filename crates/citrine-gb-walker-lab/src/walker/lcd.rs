pub const WIDTH: usize = 96;
pub const HEIGHT: usize = 64;
const PAGES: usize = 16;
const COLUMNS: usize = 128;

#[derive(Clone)]
pub struct Lcd {
    vram: Vec<u8>,
    page: usize,
    cursor: usize,
    start_line: u8,
    param: Option<u8>,
    pub contrast: u8,
    pub powered: bool,
    pub commands: Vec<u8>,
}

impl Default for Lcd {
    fn default() -> Self {
        Self {
            vram: vec![0; PAGES * COLUMNS * 2],
            page: 0,
            cursor: 0,
            start_line: 0,
            param: None,
            contrast: 0,
            powered: true,
            commands: Vec::new(),
        }
    }
}

impl Lcd {
    pub fn command(&mut self, c: u8) {
        if self.commands.len() < 4096 {
            self.commands.push(c);
        }
        if let Some(cmd) = self.param.take() {
            match cmd {
                0x40..=0x43 => self.start_line = c & 0x7F,
                0x81 => self.contrast = c & 0x3F,
                _ => {}
            }
            return;
        }
        match c {
            0x00..=0x0F => self.cursor = (((self.cursor / 2) & 0x70) | c as usize) * 2,
            0x10..=0x17 => self.cursor = (((self.cursor / 2) & 0x0F) | ((c as usize & 7) << 4)) * 2,
            0x40..=0x43 | 0x44 | 0x48 | 0x4C | 0x81 | 0x88..=0x8F | 0xF1 | 0xF6 | 0xF7 => {
                self.param = Some(c)
            }
            0xA9 => self.powered = false,
            0xE1 => self.powered = true,
            0xAE => self.powered = false,
            0xAF => self.powered = true,
            0xB0..=0xBF => self.page = (c & 0xF) as usize,
            _ => {}
        }
    }

    pub fn data(&mut self, d: u8) {
        let i = self.page * COLUMNS * 2 + self.cursor % (COLUMNS * 2);
        self.vram[i] = d;
        self.cursor = (self.cursor + 1) % (COLUMNS * 2);
    }

    pub fn shade(&self, x: usize, y: usize) -> u8 {
        let line = (y + self.start_line as usize) % (PAGES * 8);
        let base = (line / 8) * COLUMNS * 2 + x * 2;
        let bit = line % 8;
        let hi = (self.vram[base] >> bit) & 1;
        let lo = (self.vram[base + 1] >> bit) & 1;
        (hi << 1) | lo
    }

    pub fn frame(&self) -> Vec<u8> {
        (0..HEIGHT)
            .flat_map(|y| (0..WIDTH).map(move |x| (x, y)))
            .map(|(x, y)| self.shade(x, y))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_lands_at_page_and_column_with_two_planes() {
        let mut lcd = Lcd::default();
        for c in [0xB1, 0x13, 0x02] {
            lcd.command(c);
        }
        lcd.data(0x01);
        lcd.data(0x01);
        lcd.data(0x80);
        lcd.data(0x00);
        assert_eq!(lcd.shade(0x32, 8), 3);
        assert_eq!(lcd.shade(0x33, 15), 2);
        assert_eq!(lcd.shade(0x33, 8), 0);
    }

    #[test]
    fn parameter_bytes_are_not_commands() {
        let mut lcd = Lcd::default();
        lcd.command(0x81);
        lcd.command(0xB5);
        assert_eq!(lcd.contrast, 0x35);
        lcd.data(0x01);
        lcd.data(0x00);
        assert_eq!(lcd.shade(0, 0), 2);
    }
}

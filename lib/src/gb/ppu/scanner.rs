use crate::gb::ppu::types::sprite::Sprite;
use crate::gb::ppu::Ppu;

#[derive(Debug, Default)]
pub struct OamScanner {
    // Takes 2 dots to scan 1 entry in oam => 80 dots total
    dot_progress: u8,
    // Always equal or less than 10 entries
    buffer: Vec<Sprite>,
}

impl OamScanner {
    pub fn reset(&mut self) {
        self.dot_progress = 0;
        self.buffer.clear();
    }

    pub fn pop_sprite_for_x(&mut self, x: u8) -> Option<Sprite> {
        let index = self
            .buffer
            .iter()
            // ToDo: Check trigger
            .position(|sprite| x == sprite.x.saturating_sub(8))?;
        Some(self.buffer.remove(index))
    }
}

impl Ppu {
    // ToDo: Handle sprite y_flip?
    // ToDo: candidate for off-by-one errors
    /// Returns true when done
    pub fn dot_oam_scan(&mut self) -> bool {
        if self.scanner.dot_progress >= 80 {
            self.scanner.reset();
        };

        self.scanner.dot_progress += 1;

        // Don't fetch sprite on odd dot
        if (self.scanner.dot_progress & 1) != 0 {
            return false;
        };

        // Don't ever fetch more than 10 sprites
        if self.scanner.buffer.len() >= 10 {
            return self.scanner.dot_progress >= 80;
        }

        let index = (self.scanner.dot_progress - 1) / 2;
        let sprite = self.fetch_sprite(index);

        if sprite.x > 0
            && self.ly + 16 >= sprite.y
            && self.ly + 16 < sprite.y + self.lcdc.sprite_height()
        {
            self.scanner.buffer.push(sprite);
        }

        self.scanner.dot_progress == 80
    }

    fn fetch_sprite(&self, index: u8) -> Sprite {
        let i = (index % 40) as usize * 4;
        let bytes: [u8; 4] = self.oam[i..i + 4].try_into().unwrap();
        let mut sprite: Sprite = bytes.into();
        sprite.oam_index = index;
        sprite
    }
}

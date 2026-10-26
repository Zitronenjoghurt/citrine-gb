use crate::gb::ppu::Ppu;

#[derive(Debug, Default, Copy, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ScannedObject {
    pub x: u8,
    pub y: u8,
    pub oam_index: u8,
}

#[derive(Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct OamScanner {
    buffer: [ScannedObject; 10],
    len: u8,
    next: u8,
}

impl OamScanner {
    pub fn reset(&mut self) {
        self.len = 0;
        self.next = 0;
    }

    pub fn peek(&self) -> Option<&ScannedObject> {
        if self.next < self.len {
            Some(&self.buffer[self.next as usize])
        } else {
            None
        }
    }

    pub fn advance(&mut self) {
        self.next += 1;
    }

    pub fn retreat(&mut self) {
        self.next = self.next.saturating_sub(1);
    }

    fn insert(&mut self, object: ScannedObject) {
        let len = self.len as usize;
        let at = self.buffer[..len]
            .iter()
            .position(|o| o.x > object.x)
            .unwrap_or(len);
        self.buffer.copy_within(at..len, at + 1);
        self.buffer[at] = object;
        self.len += 1;
    }
}

impl Ppu {
    pub fn scan_oam_entry(&mut self, index: u8) {
        if self.scanner.len >= 10 {
            return;
        }
        let i = index as usize * 4;
        let y = self.oam[i];
        let x = self.oam[i + 1];
        let top = y as i16 - 16;
        let line = self.line as i16;
        if top <= line && line < top + self.lcdc.sprite_height() as i16 {
            self.scanner.insert(ScannedObject {
                x,
                y,
                oam_index: index,
            });
        }
    }

    pub fn object_at_x0_pending(&self) -> bool {
        self.objects_fetchable() && self.scanner.peek().is_some_and(|o| o.x == 0)
    }

    pub fn objects_fetchable(&self) -> bool {
        self.lcdc.obj_enable || self.model.is_cgb()
    }
}

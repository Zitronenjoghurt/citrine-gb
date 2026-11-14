use crate::gb::ppu::Ppu;

#[derive(Debug, Default, Copy, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ScannedObject {
    pub x: u8,
    pub y: u8,
    pub oam_index: u8,
}

#[derive(Debug, Clone, Default)]
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

    pub fn holds(&self, oam_index: u8) -> bool {
        self.buffer[..self.len as usize]
            .iter()
            .any(|o| o.oam_index == oam_index)
    }

    pub fn retreat(&mut self) {
        self.next = self.next.saturating_sub(1);
    }

    pub fn rewind(&mut self) {
        self.next = 0;
    }

    pub fn is_rewound(&self) -> bool {
        self.next == 0
    }

    pub fn next_at_or_after(&self, x: u8) -> Option<u8> {
        self.buffer[self.next as usize..self.len as usize]
            .iter()
            .map(|o| o.x)
            .find(|&ox| ox >= x)
    }

    pub fn skip_before(&mut self, x: u8) {
        while self.peek().is_some_and(|o| o.x < x) {
            self.next += 1;
        }
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

#[derive(Debug, Clone)]
pub struct OamLines {
    height: u8,
    masks: [u64; 144],
}

impl Default for OamLines {
    fn default() -> Self {
        Self {
            height: 0,
            masks: [0; 144],
        }
    }
}

impl OamLines {
    pub fn invalidate(&mut self) {
        self.height = 0;
    }

    fn line(&mut self, oam: &[u8], line: u8, height: u8) -> u64 {
        if self.height != height {
            self.rebuild(oam, height);
        }
        self.masks[line as usize]
    }

    #[inline(never)]
    fn rebuild(&mut self, oam: &[u8], height: u8) {
        self.masks = [0; 144];
        for index in 0..40 {
            let y = oam[index * 4] as usize;
            let first = y.saturating_sub(16);
            let end = (y + height as usize).saturating_sub(16).min(144);
            for mask in self.masks.iter_mut().take(end).skip(first) {
                *mask |= 1 << index;
            }
        }
        self.height = height;
    }
}

impl Ppu {
    pub fn scan_oam_entry(&mut self, index: u8) {
        self.scan_oam_entries(index..index + 1);
    }

    pub fn scan_oam_entries(&mut self, entries: std::ops::Range<u8>) {
        let line = self.line as i16;
        let height = self.lcdc.sprite_height() as i16;
        for index in entries {
            if self.scanner.len >= 10 {
                return;
            }
            let i = index as usize * 4;
            let y = self.oam[i];
            let top = y as i16 - 16;
            if top <= line && line < top + height {
                self.scanner.insert(ScannedObject {
                    x: self.oam[i + 1],
                    y,
                    oam_index: index,
                });
            }
        }
    }

    pub fn scan_oam_line(&mut self) {
        debug_assert!(self.line < 144);
        let height = self.lcdc.sprite_height();
        let hits = self.oam_lines.line(&self.oam, self.line, height);
        self.insert_line_hits(hits);
    }

    pub fn scan_oam_lines(&mut self, entries: std::ops::Range<u8>) {
        if self.line >= 144 {
            self.scan_oam_entries(entries);
            return;
        }
        let below = |n: u8| (1u64 << n) - 1;
        let height = self.lcdc.sprite_height();
        let hits = self.oam_lines.line(&self.oam, self.line, height)
            & below(entries.end)
            & !below(entries.start);
        self.insert_line_hits(hits);
    }

    fn insert_line_hits(&mut self, mut hits: u64) {
        while hits != 0 && self.scanner.len < 10 {
            let index = hits.trailing_zeros() as u8;
            hits &= hits - 1;
            let i = index as usize * 4;
            self.scanner.insert(ScannedObject {
                x: self.oam[i + 1],
                y: self.oam[i],
                oam_index: index,
            });
        }
    }

    pub fn objects_fetchable(&self) -> bool {
        self.lcdc.obj_enable || self.model.is_cgb()
    }
}

#[cfg(test)]
mod tests {
    use crate::WriteMemory;
    use crate::gb::GbModel;
    use crate::gb::ppu::Ppu;

    #[test]
    fn line_masks_find_what_a_full_scan_finds_after_oam_and_size_changes() {
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for model in [GbModel::Dmg, GbModel::Cgb] {
            let mut ppu = Ppu::new(model);
            for _ in 0..3000 {
                match next() % 4 {
                    0 => ppu.write_naive(0xFE00 + (next() % 160) as u16, next() as u8),
                    1 => ppu.lcdc.obj_size = next() & 1 == 1,
                    _ => {}
                }
                ppu.line = (next() % 144) as u8;
                let start = (next() % 41) as u8;
                let end = start + (next() % (41 - start as u64)) as u8;
                let mut full = ppu.clone();
                full.scanner.reset();
                full.scan_oam_entries(start..end);
                ppu.scanner.reset();
                ppu.scan_oam_lines(start..end);
                assert_eq!(
                    format!("{:?}", ppu.scanner),
                    format!("{:?}", full.scanner),
                    "{model:?} line {} entries {start}..{end}",
                    ppu.line
                );
            }
        }
    }
}

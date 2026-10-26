#[derive(Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Hdma {
    source: u16,
    destination: u16,
    blocks_left: u8,
    hblank_mode: bool,
    pending: bool,
    hblank_seen_in_halt: bool,
}

impl Hdma {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read_hdma5(&self) -> u8 {
        let active = if self.pending || self.hblank_mode {
            0x00
        } else {
            0x80
        };
        active | (self.blocks_left.wrapping_sub(1) & 0x7F)
    }

    pub fn write(&mut self, addr: u16, value: u8, in_hblank: bool) {
        match addr {
            0xFF51 => {
                self.source = (self.source & 0x00F0) | ((value as u16) << 8);
                if self.source >= 0xE000 {
                    self.source |= 0xF000;
                }
            }
            0xFF52 => self.source = (self.source & 0xFF00) | (value as u16 & 0xF0),
            0xFF53 => self.destination = (self.destination & 0x00F0) | ((value as u16) << 8),
            0xFF54 => self.destination = (self.destination & 0xFF00) | (value as u16 & 0xF0),
            0xFF55 => {
                self.blocks_left = (value & 0x7F) + 1;
                if value & 0x80 == 0 && self.hblank_mode {
                    self.hblank_mode = false;
                    return;
                }
                self.hblank_mode = value & 0x80 != 0;
                self.pending = !self.hblank_mode || in_hblank;
            }
            _ => {}
        }
    }

    pub fn hblank_started(&mut self, halted: bool) {
        if !self.hblank_mode {
            return;
        }
        if halted {
            self.hblank_seen_in_halt = true;
        } else {
            self.pending = true;
        }
    }

    pub fn left_halt(&mut self, in_hblank: bool) {
        if self.hblank_mode && self.hblank_seen_in_halt && in_hblank {
            self.pending = true;
        }
        self.hblank_seen_in_halt = false;
    }

    pub fn lcd_disabled(&mut self) {
        if self.hblank_mode {
            self.pending = true;
        }
    }

    pub fn is_pending(&self) -> bool {
        self.pending
    }

    pub fn next_transfer(&mut self) -> (u16, u16) {
        let transfer = (self.source, 0x8000 | (self.destination & 0x1FFF));
        self.source = self.source.wrapping_add(1);
        self.destination = self.destination.wrapping_add(1);
        transfer
    }

    pub fn block_done(&mut self) -> bool {
        self.blocks_left = self.blocks_left.wrapping_sub(1);
        if self.blocks_left == 0 || self.destination == 0 {
            self.pending = false;
            self.hblank_mode = false;
            return true;
        }
        if self.hblank_mode {
            self.pending = false;
            return true;
        }
        false
    }
}

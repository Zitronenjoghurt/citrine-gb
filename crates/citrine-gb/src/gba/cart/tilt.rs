use super::sensor::scale;

pub const START: usize = 0x8000;
pub const END: usize = 0x8600;
const ARM: usize = 0x8000;
const LATCH: usize = 0x8100;
const X_LOW: usize = 0x8200;
const X_HIGH: usize = 0x8300;
const Y_LOW: usize = 0x8400;
const Y_HIGH: usize = 0x8500;
const ARM_VALUE: u8 = 0x55;
const LATCH_VALUE: u8 = 0xAA;
const READY: u8 = 0x80;
const X_RANGE: (f32, f32, f32) = (687.0, 914.0, 1143.0);
const Y_RANGE: (f32, f32, f32) = (707.0, 928.0, 1152.0);

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Tilt {
    x: u16,
    y: u16,
    latched: (u16, u16),
    armed: bool,
}

impl Default for Tilt {
    fn default() -> Self {
        let centre = (X_RANGE.1 as u16, Y_RANGE.1 as u16);
        Self {
            x: centre.0,
            y: centre.1,
            latched: centre,
            armed: false,
        }
    }
}

impl Tilt {
    pub fn set(&mut self, x: f32, y: f32) {
        self.x = scale(x, X_RANGE.0, X_RANGE.1, X_RANGE.2);
        self.y = scale(y, Y_RANGE.0, Y_RANGE.1, Y_RANGE.2);
    }

    pub fn read8(&self, offset: usize) -> Option<u8> {
        let (x, y) = self.latched;
        match offset & !0xFF {
            X_LOW => Some(x as u8),
            X_HIGH => Some(((x >> 8) as u8 & 0x0F) | READY),
            Y_LOW => Some(y as u8),
            Y_HIGH => Some((y >> 8) as u8 & 0x0F),
            _ => None,
        }
    }

    pub fn write8(&mut self, offset: usize, value: u8) {
        match (offset & !0xFF, value) {
            (ARM, ARM_VALUE) => self.armed = true,
            (LATCH, LATCH_VALUE) if self.armed => {
                self.armed = false;
                self.latched = (self.x, self.y);
            }
            _ => self.armed = false,
        }
    }
}

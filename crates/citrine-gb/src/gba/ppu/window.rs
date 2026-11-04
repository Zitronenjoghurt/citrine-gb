use super::WIDTH;

pub const ALL_LAYERS: u8 = 0x3F;
const WIN0: u16 = 1 << 13;
const WIN1: u16 = 1 << 14;
const OBJ_WINDOW: u16 = 1 << 15;
const CONTROL_MASK: u16 = 0x3F3F;

#[derive(Debug, Default, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Windows {
    horizontal: [u16; 2],
    vertical: [u16; 2],
    inside: u16,
    outside: u16,
    active: [bool; 2],
}

impl Windows {
    pub fn write(&mut self, offset: usize, value: u16, mask: u16) {
        let register = match offset {
            0x0 | 0x2 => &mut self.horizontal[offset / 2],
            0x4 | 0x6 => &mut self.vertical[(offset - 4) / 2],
            0x8 => &mut self.inside,
            _ => &mut self.outside,
        };
        *register = (*register & !mask) | (value & mask);
        self.inside &= CONTROL_MASK;
        self.outside &= CONTROL_MASK;
    }

    pub fn start_line(&mut self, y: u16) {
        for (active, vertical) in self.active.iter_mut().zip(self.vertical) {
            if y == vertical >> 8 {
                *active = true;
            }
            if y == vertical & 0xFF {
                *active = false;
            }
        }
    }

    pub fn read(&self, offset: usize) -> u16 {
        match offset {
            0x8 => self.inside,
            0xA => self.outside,
            _ => 0,
        }
    }

    pub fn line(&self, dispcnt: u16, object: &[bool; WIDTH], out: &mut [u8; WIDTH]) {
        if dispcnt & (WIN0 | WIN1 | OBJ_WINDOW) == 0 {
            out.fill(ALL_LAYERS);
            return;
        }
        out.fill(layers(self.outside));
        if dispcnt & OBJ_WINDOW != 0 {
            let enabled = layers(self.outside >> 8);
            for (mask, &inside) in out.iter_mut().zip(object) {
                if inside {
                    *mask = enabled;
                }
            }
        }
        for (window, flag, control) in [(1, WIN1, self.inside >> 8), (0, WIN0, self.inside)] {
            if dispcnt & flag == 0 {
                continue;
            }
            if !self.active[window] {
                continue;
            }
            let (left, right) = span(self.horizontal[window], WIDTH);
            if left < right {
                out[left..right].fill(layers(control));
            }
        }
    }
}

fn layers(control: u16) -> u8 {
    (control & 0x3F) as u8
}

fn span(register: u16, limit: usize) -> (usize, usize) {
    let start = usize::from(register >> 8).min(limit);
    let end = usize::from(register & 0xFF);
    if start > end || end > limit {
        (start, limit)
    } else {
        (start, end)
    }
}

#[cfg(test)]
mod tests;

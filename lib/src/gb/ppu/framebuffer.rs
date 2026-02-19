use crate::gb::ppu::color::RGBA;

#[derive(Debug)]
pub struct Framebuffer([u8; 160 * 144 * 4]);

impl Default for Framebuffer {
    fn default() -> Self {
        Self([0; 160 * 144 * 4])
    }
}

impl Framebuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, index: usize, color: RGBA) {
        if index >= 23040 {
            return;
        }
        self.0[index * 4] = color.r();
        self.0[index * 4 + 1] = color.g();
        self.0[index * 4 + 2] = color.b();
        self.0[index * 4 + 3] = color.a();
    }

    pub fn clear(&mut self) {
        self.0.fill(0);
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

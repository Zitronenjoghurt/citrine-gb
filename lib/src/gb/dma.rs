use crate::gb::GbModel;

#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DmaController {
    pub active: bool,
    pub source: u8,
    pub progress: u8,
    pub model: GbModel,
    #[cfg_attr(feature = "serde", serde(default))]
    pub delay: u8,
    #[cfg_attr(feature = "serde", serde(default))]
    pub bus_value: u8,
}

impl DmaController {
    pub fn new(model: GbModel) -> Self {
        Self {
            active: false,
            source: if model == GbModel::Cgb { 0x00 } else { 0xFF },
            progress: 0,
            model,
            delay: 0,
            bus_value: 0xFF,
        }
    }

    pub fn start(&mut self, source: u8) {
        self.active = self.active && self.progress < 160;
        self.delay = 2;
        self.source = source;
        self.progress = 0;
    }

    /// Returns a byte copy source and target address if a DMA transfer is ongoing, otherwise None.
    pub fn cycle(&mut self) -> Option<(u16, u16)> {
        if self.delay > 0 {
            self.delay -= 1;
            if self.delay > 0 {
                return None;
            }
            self.active = true;
        }

        if !self.active {
            return None;
        };

        if self.progress >= 160 {
            self.active = false;
            return None;
        }

        let source = (self.source as u16) << 8 | self.progress as u16;
        let target = 0xFE00 | self.progress as u16;

        self.progress += 1;

        Some((source, target))
    }

    pub fn source_address(&self, source: u16) -> u16 {
        if source >= 0xE000 && self.model.is_dmg() {
            source & !0x2000
        } else {
            source
        }
    }

    pub fn cpu_conflicts(&self, addr: u16) -> bool {
        if !self.active || addr >= 0xFF00 {
            return false;
        }

        if addr >= 0xFE00 {
            return true;
        }

        if self.delay > 0 {
            return false;
        }

        let src = self.source;
        let source_on_vram = (0x80..=0x9F).contains(&src);
        let addr_on_vram = (0x8000..=0x9FFF).contains(&addr);

        if self.model.is_dmg() {
            return source_on_vram == addr_on_vram;
        };

        let source_on_cartridge = src <= 0x7F || (0xA0..=0xBF).contains(&src);
        let source_on_wram = (0xC0..=0xDF).contains(&src);

        let addr_on_cartridge = addr <= 0x7FFF || (0xA000..=0xBFFF).contains(&addr);
        let addr_on_wram = (0xC000..=0xDFFF).contains(&addr);

        (source_on_vram && addr_on_vram)
            || (source_on_cartridge && addr_on_cartridge)
            || (source_on_wram && addr_on_wram)
    }

    pub fn soft_reset(&mut self) {
        *self = Self::new(self.model);
    }
}

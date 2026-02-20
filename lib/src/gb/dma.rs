#[derive(Debug)]
pub struct DmaController {
    pub active: bool,
    pub source: u8,
    pub progress: u8,
    pub cgb: bool,
}

impl DmaController {
    pub fn new(cgb: bool) -> Self {
        if cgb {
            Self {
                active: false,
                source: 0x00,
                progress: 0,
                cgb: true,
            }
        } else {
            Self {
                active: false,
                source: 0xFF,
                progress: 0,
                cgb: false,
            }
        }
    }

    pub fn start(&mut self, source: u8) {
        self.active = true;
        self.source = source;
        self.progress = 0;
    }

    /// Returns a byte copy source and target address if a DMA transfer is ongoing, otherwise None.
    pub fn cycle(&mut self) -> Option<(u16, u16)> {
        if self.progress >= 160 {
            self.active = false;
        }

        if !self.active {
            return None;
        };

        let source = (self.source as u16) << 8 | self.progress as u16;
        let target = 0xFE00 | self.progress as u16;

        self.progress += 1;

        Some((source, target))
    }

    pub fn cpu_conflicts(&self, addr: u16) -> bool {
        if !self.active {
            return false;
        }

        // HRAM
        if (0xFF80..=0xFFFE).contains(&addr) {
            return false;
        };

        // OAM
        if (0xFE00..=0xFE9F).contains(&addr) {
            return true;
        };

        if !self.cgb {
            return true;
        };

        // CGB: only the bus that DMA is reading from is blocked
        // ToDo: check if this is correctly conflicting on CGB
        let src = self.source;
        let source_on_cartridge = src <= 0x7F || (0xA0..=0xBF).contains(&src);
        let source_on_wram = (0xC0..=0xDF).contains(&src);

        let addr_on_cartridge = addr <= 0x7FFF || (0xA000..=0xBFFF).contains(&addr);
        let addr_on_wram = (0xC000..=0xDFFF).contains(&addr);

        (source_on_cartridge && addr_on_cartridge) || (source_on_wram && addr_on_wram)
    }
}

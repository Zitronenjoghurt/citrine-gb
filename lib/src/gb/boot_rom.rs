#[derive(Debug, Clone)]
pub struct BootRom {
    pub rom: Vec<u8>,
    pub mounted: bool,
}

impl Default for BootRom {
    fn default() -> Self {
        Self {
            rom: vec![],
            mounted: true,
        }
    }
}

impl BootRom {
    /// The address right outside the boot ROM
    pub fn boundary_address(&self) -> u16 {
        self.rom.len() as u16
    }

    pub fn soft_reset(&mut self) {
        self.mounted = true;
    }
}

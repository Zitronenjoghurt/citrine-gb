use crate::gb::GbModel;

#[derive(Debug, Default, Clone)]
pub struct BootRom {
    pub rom: Vec<u8>,
    pub mounted: bool,
}

impl BootRom {
    pub fn new(rom: Vec<u8>, model: GbModel) -> Self {
        let mut boot_rom = Self {
            rom,
            mounted: false,
        };
        boot_rom.soft_reset(model);
        boot_rom
    }

    pub fn fits(&self, model: GbModel) -> bool {
        match model {
            GbModel::Dmg => self.rom.len() == 0x100,
            GbModel::Cgb => self.rom.len() == 0x900,
        }
    }

    pub fn maps(&self, addr: u16) -> bool {
        self.mounted && (addr as usize) < self.rom.len() && !(0x100..0x200).contains(&addr)
    }

    pub fn soft_reset(&mut self, model: GbModel) {
        self.mounted = self.fits(model);
    }
}

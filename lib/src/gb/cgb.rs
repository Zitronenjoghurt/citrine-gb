use crate::gb::GbModel;

#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CgbControl {
    model: GbModel,
    pub cgb_mode: bool,
    pub double_speed: bool,
    half_cycle: bool,
    key0: u8,
    key1: u8,
    rp: u8,
    ff72: u8,
    ff73: u8,
    ff74: u8,
    ff75: u8,
    #[cfg_attr(feature = "serde", serde(default))]
    pub div_reset_pending: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub div_reset_lookahead: bool,
}

impl Default for CgbControl {
    fn default() -> Self {
        Self::new(GbModel::Dmg)
    }
}

impl CgbControl {
    pub fn new(model: GbModel) -> Self {
        Self {
            model,
            cgb_mode: model.is_cgb(),
            double_speed: false,
            half_cycle: false,
            key0: 0x00,
            key1: 0x00,
            rp: 0x00,
            ff72: 0x00,
            ff73: 0x00,
            ff74: 0x00,
            ff75: 0x00,
            div_reset_pending: false,
            div_reset_lookahead: false,
        }
    }

    pub fn normal_speed_cycle(&mut self) -> bool {
        if !self.double_speed {
            return true;
        }
        self.half_cycle = !self.half_cycle;
        !self.half_cycle
    }

    pub fn ensure_model(&mut self, model: GbModel) {
        if self.model != model {
            *self = Self::new(model);
        }
    }

    pub fn set_dmg_compatibility(&mut self) {
        if self.model.is_cgb() {
            self.key0 = 0x04;
            self.cgb_mode = false;
        }
    }

    pub fn write_key0(&mut self, value: u8) {
        if self.model.is_cgb() {
            self.key0 = value;
            self.cgb_mode = value & 0x0C == 0;
        }
    }

    pub fn speed_switch_armed(&self) -> bool {
        self.cgb_mode && self.key1 & 0x01 != 0
    }

    pub fn switch_speed(&mut self, div_reset_lookahead: bool) {
        self.double_speed = !self.double_speed;
        self.key1 = 0x00;
        self.div_reset_pending = true;
        self.div_reset_lookahead = div_reset_lookahead;
    }

    pub fn read(&self, addr: u16) -> u8 {
        if self.model.is_dmg() {
            return 0xFF;
        }

        match addr {
            0xFF4D if self.cgb_mode => ((self.double_speed as u8) << 7) | 0x7E | (self.key1 & 0x01),
            0xFF56 if self.cgb_mode => (self.rp & 0xC1) | 0x2E,
            0xFF72 => self.ff72,
            0xFF73 => self.ff73,
            0xFF74 if self.cgb_mode => self.ff74,
            0xFF75 => self.ff75 | 0x8F,
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        if self.model.is_dmg() {
            return;
        }

        match addr {
            0xFF4D if self.cgb_mode => self.key1 = value & 0x01,
            0xFF56 if self.cgb_mode => self.rp = value & 0xC1,
            0xFF72 => self.ff72 = value,
            0xFF73 => self.ff73 = value,
            0xFF74 if self.cgb_mode => self.ff74 = value,
            0xFF75 => self.ff75 = value & 0x70,
            _ => {}
        }
    }
}

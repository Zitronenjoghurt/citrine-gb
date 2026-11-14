const READ: u8 = 1 << 7;
const INDEX: u8 = 0x7F;

const CONTROL: u8 = 0;
const BATTERY: u8 = 1;
const MIC_ENABLE: u8 = 2;
const MIC_GAIN: u8 = 3;
const BACKLIGHT_LEVEL: u8 = 4;

const CONTROL_WRITABLE: u8 = 0x7F;
const LITE_CONTROL_WRITABLE: u8 = 0x7D;
const SOUND_AMP: u8 = 1 << 0;
const LOWER_BACKLIGHT: u8 = 1 << 2;
const UPPER_BACKLIGHT: u8 = 1 << 3;
const POWER_OFF: u8 = 1 << 6;
const BATTERY_LOW: u8 = 1 << 0;
const LEVEL: u8 = 0x03;
const FORCE_MAX: u8 = 1 << 2;
const EXTERNAL_POWER: u8 = 1 << 3;
const LEVEL_FIXED: u8 = 0x40;
const RESET_CONTROL: u8 = SOUND_AMP | LOWER_BACKLIGHT | UPPER_BACKLIGHT;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PowerModel {
    #[default]
    Ds,
    Lite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PowerManager {
    model: PowerModel,
    control: u8,
    battery_low: bool,
    mic_enable: u8,
    mic_gain: u8,
    level: u8,
    external_power: bool,
    command: Option<u8>,
    lid_closed: bool,
    lid_interrupt: bool,
}

impl Default for PowerManager {
    fn default() -> Self {
        Self::new(PowerModel::Ds)
    }
}

impl PowerManager {
    pub fn new(model: PowerModel) -> Self {
        Self {
            model,
            control: RESET_CONTROL,
            battery_low: false,
            mic_enable: 0,
            mic_gain: 0,
            level: LEVEL,
            external_power: false,
            command: None,
            lid_closed: false,
            lid_interrupt: false,
        }
    }

    pub fn transfer(&mut self, byte: u8) -> u8 {
        let Some(command) = self.command else {
            self.command = Some(byte);
            return 0;
        };
        let register = self.register(command & INDEX);
        if command & READ != 0 {
            self.read(register)
        } else {
            self.write(register, byte);
            0
        }
    }

    pub fn deselect(&mut self) {
        self.command = None;
    }

    pub fn sound_enabled(&self) -> bool {
        self.control & SOUND_AMP != 0
    }

    pub fn lower_backlight(&self) -> bool {
        self.control & LOWER_BACKLIGHT != 0
    }

    pub fn upper_backlight(&self) -> bool {
        self.control & UPPER_BACKLIGHT != 0
    }

    pub fn backlight_level(&self) -> u8 {
        match self.model {
            PowerModel::Ds => LEVEL,
            PowerModel::Lite => self.read(BACKLIGHT_LEVEL) & LEVEL,
        }
    }

    pub fn powered_off(&self) -> bool {
        self.control & POWER_OFF != 0
    }

    pub fn mic_amplifier(&self) -> Option<u8> {
        (self.mic_enable != 0).then_some(self.mic_gain)
    }

    pub fn set_battery_low(&mut self, low: bool) {
        self.battery_low = low;
    }

    pub fn set_external_power(&mut self, present: bool) {
        self.external_power = present;
    }

    pub fn lid_closed(&self) -> bool {
        self.lid_closed
    }

    pub fn set_lid_closed(&mut self, closed: bool) {
        if self.lid_closed && !closed {
            self.lid_interrupt = true;
        }
        self.lid_closed = closed;
    }

    pub fn take_lid_interrupt(&mut self) -> bool {
        std::mem::take(&mut self.lid_interrupt)
    }

    fn register(&self, index: u8) -> u8 {
        match self.model {
            PowerModel::Ds => index & 3,
            PowerModel::Lite => (index & 7).min(BACKLIGHT_LEVEL),
        }
    }

    fn read(&self, register: u8) -> u8 {
        match register {
            CONTROL => self.control,
            BATTERY => u8::from(self.battery_low) * BATTERY_LOW,
            MIC_ENABLE => self.mic_enable,
            MIC_GAIN => self.mic_gain,
            _ => {
                let external = if self.external_power {
                    EXTERNAL_POWER
                } else {
                    0
                };
                let forced = self.level & FORCE_MAX != 0 && self.external_power;
                let level = if forced { LEVEL } else { self.level & LEVEL };
                LEVEL_FIXED | external | (self.level & FORCE_MAX) | level
            }
        }
    }

    fn write(&mut self, register: u8, value: u8) {
        match register {
            CONTROL => {
                let writable = match self.model {
                    PowerModel::Ds => CONTROL_WRITABLE,
                    PowerModel::Lite => LITE_CONTROL_WRITABLE,
                };
                self.control = value & writable;
            }
            MIC_ENABLE => self.mic_enable = value & 1,
            MIC_GAIN => self.mic_gain = value & 3,
            BACKLIGHT_LEVEL => self.level = value & (LEVEL | FORCE_MAX),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(power: &mut PowerManager, index: u8) -> u8 {
        power.transfer(READ | index);
        let value = power.transfer(0);
        power.deselect();
        value
    }

    fn write(power: &mut PowerManager, index: u8, value: u8) {
        power.transfer(index);
        power.transfer(value);
        power.deselect();
    }

    #[test]
    fn control_starts_with_sound_and_both_backlights_on() {
        let mut power = PowerManager::default();
        assert_eq!(read(&mut power, CONTROL), RESET_CONTROL);
        assert!(power.sound_enabled());
        assert!(power.lower_backlight() && power.upper_backlight());
        assert!(!power.powered_off());
    }

    #[test]
    fn backlights_follow_the_control_bits() {
        let mut power = PowerManager::default();
        write(&mut power, CONTROL, UPPER_BACKLIGHT);
        assert!(power.upper_backlight());
        assert!(!power.lower_backlight());
        assert!(!power.sound_enabled());
        write(&mut power, CONTROL, LOWER_BACKLIGHT);
        assert!(power.lower_backlight());
        assert!(!power.upper_backlight());
    }

    #[test]
    fn bit_6_powers_the_console_off() {
        let mut power = PowerManager::default();
        write(&mut power, CONTROL, POWER_OFF);
        assert!(power.powered_off());
        assert_eq!(read(&mut power, CONTROL) & 0x80, 0);
    }

    #[test]
    fn battery_reads_low_and_ignores_writes() {
        let mut power = PowerManager::default();
        assert_eq!(read(&mut power, BATTERY), 0);
        power.set_battery_low(true);
        write(&mut power, BATTERY, 0);
        assert_eq!(read(&mut power, BATTERY), 1);
    }

    #[test]
    fn mic_amplifier_enable_and_gain() {
        let mut power = PowerManager::default();
        assert_eq!(power.mic_amplifier(), None);
        write(&mut power, MIC_GAIN, 0xFE);
        write(&mut power, MIC_ENABLE, 0xFF);
        assert_eq!(read(&mut power, MIC_GAIN), 2);
        assert_eq!(read(&mut power, MIC_ENABLE), 1);
        assert_eq!(power.mic_amplifier(), Some(2));
    }

    #[test]
    fn the_ds_mirrors_registers_every_four() {
        let mut power = PowerManager::new(PowerModel::Ds);
        write(&mut power, 0x7E, 3);
        assert_eq!(read(&mut power, MIC_ENABLE), 1);
        assert_eq!(read(&mut power, 4), read(&mut power, CONTROL));
        assert_eq!(power.backlight_level(), 3);
    }

    #[test]
    fn the_lite_has_backlight_levels_in_register_4() {
        let mut power = PowerManager::new(PowerModel::Lite);
        write(&mut power, BACKLIGHT_LEVEL, 1);
        assert_eq!(read(&mut power, BACKLIGHT_LEVEL), LEVEL_FIXED | 1);
        assert_eq!(power.backlight_level(), 1);
        assert_eq!(read(&mut power, 6), read(&mut power, BACKLIGHT_LEVEL));
        assert_eq!(read(&mut power, 9), read(&mut power, BATTERY));
    }

    #[test]
    fn the_lite_forces_maximum_on_external_power() {
        let mut power = PowerManager::new(PowerModel::Lite);
        write(&mut power, BACKLIGHT_LEVEL, FORCE_MAX);
        assert_eq!(power.backlight_level(), 0);
        power.set_external_power(true);
        assert_eq!(
            read(&mut power, BACKLIGHT_LEVEL),
            LEVEL_FIXED | EXTERNAL_POWER | FORCE_MAX | 3
        );
        assert_eq!(power.backlight_level(), 3);
    }

    #[test]
    fn the_lite_has_no_mute_bit() {
        let mut power = PowerManager::new(PowerModel::Lite);
        write(&mut power, CONTROL, 0xFF);
        assert_eq!(read(&mut power, CONTROL), 0x7D);
    }

    #[test]
    fn opening_the_lid_raises_the_interrupt() {
        let mut power = PowerManager::default();
        power.set_lid_closed(true);
        assert!(power.lid_closed());
        assert!(!power.take_lid_interrupt());
        power.set_lid_closed(false);
        assert!(!power.lid_closed());
        assert!(power.take_lid_interrupt());
        assert!(!power.take_lid_interrupt());
        power.set_lid_closed(false);
        assert!(!power.take_lid_interrupt());
    }

    #[test]
    fn a_read_keeps_repeating_the_register() {
        let mut power = PowerManager::default();
        power.transfer(READ | CONTROL);
        assert_eq!(power.transfer(0), RESET_CONTROL);
        assert_eq!(power.transfer(0), RESET_CONTROL);
    }
}

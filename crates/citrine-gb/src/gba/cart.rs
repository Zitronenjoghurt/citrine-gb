mod date;
mod detect;
mod gpio;
mod rtc;
mod sensor;
mod tilt;

pub use date::DateTime;
use gpio::Gpio;
use rtc::Rtc;
use sensor::{Gyro, Solar};
use tilt::Tilt;

const RUMBLE: u8 = 1 << 3;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Devices {
    pub rtc: bool,
    pub solar: bool,
    pub gyro: bool,
    pub rumble: bool,
    pub tilt: bool,
}

impl Devices {
    pub fn uses_gpio(&self) -> bool {
        self.rtc || self.solar || self.gyro || self.rumble
    }
}

#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Cart {
    devices: Devices,
    gpio: Gpio,
    rtc: Rtc,
    gyro: Gyro,
    solar: Solar,
    tilt: Tilt,
    rumble: bool,
    rumbled: bool,
}

impl Cart {
    pub fn for_rom(rom: &[u8]) -> Self {
        Self {
            devices: Devices::detect(rom),
            ..Self::default()
        }
    }

    pub fn devices(&self) -> Devices {
        self.devices
    }

    pub fn clock(&self) -> DateTime {
        self.rtc.date_time()
    }

    pub fn set_clock(&mut self, unix: i64) {
        self.rtc.set_unix(unix);
    }

    pub fn set_turn(&mut self, rate: f32) {
        self.gyro.set_turn(rate);
    }

    pub fn set_sunlight(&mut self, light: f32) {
        self.solar.set_sunlight(light);
    }

    pub fn set_tilt(&mut self, x: f32, y: f32) {
        self.tilt.set(x, y);
    }

    #[cfg(feature = "persistence")]
    pub fn clock_footer(&self, now: u64) -> Option<crate::persistence::seiko_footer::SeikoFooter> {
        self.devices.rtc.then(|| self.rtc.footer(now))
    }

    #[cfg(feature = "persistence")]
    pub fn load_clock(&mut self, footer: &crate::persistence::seiko_footer::SeikoFooter, now: u64) {
        self.rtc.load(footer, now);
    }

    pub fn tick(&mut self, cycles: u32) {
        if self.devices.rtc {
            self.rtc.tick(cycles);
        }
    }

    pub fn take_rumble(&mut self) -> bool {
        let rumbled = self.rumbled;
        self.rumbled = self.rumble;
        rumbled
    }

    pub fn in_gpio(offset: usize) -> bool {
        (gpio::DATA..gpio::END).contains(&offset)
    }

    pub fn shows_gpio(&self, offset: usize) -> bool {
        Self::in_gpio(offset) && self.gpio.readable() && self.devices.uses_gpio()
    }

    pub fn read_gpio8(&self, offset: usize) -> Option<u8> {
        if !self.shows_gpio(offset) {
            return None;
        }
        let half = self.gpio.read16(offset & !1, self.inputs());
        Some(half.to_le_bytes()[offset & 1])
    }

    pub fn write_gpio16(&mut self, offset: usize, value: u16) {
        if !Self::in_gpio(offset) {
            return;
        }
        if !self.devices.uses_gpio() && !self.devices.tilt {
            self.devices.rtc = true;
        }
        self.gpio.write16(offset, value);
        self.sync();
    }

    pub fn in_tilt(&self, offset: usize) -> bool {
        self.devices.tilt && (tilt::START..tilt::END).contains(&offset)
    }

    pub fn read_tilt(&self, offset: usize) -> Option<u8> {
        self.tilt.read8(offset)
    }

    pub fn write_tilt(&mut self, offset: usize, value: u8) {
        self.tilt.write8(offset, value);
    }

    fn inputs(&self) -> u8 {
        let mut inputs = 0;
        if self.devices.rtc {
            inputs |= self.rtc.output();
        }
        if self.devices.gyro {
            inputs |= self.gyro.output();
        }
        if self.devices.solar {
            inputs |= self.solar.output();
        }
        inputs
    }

    fn sync(&mut self) {
        let pins = self.gpio.pins();
        if self.devices.rtc {
            self.rtc.update(pins);
        }
        if self.devices.gyro {
            self.gyro.update(pins);
        }
        if self.devices.solar {
            self.solar.update(pins);
        }
        if self.devices.rumble {
            self.rumble = pins & RUMBLE != 0;
            self.rumbled |= self.rumble;
        }
    }
}

#[cfg(test)]
mod tests;

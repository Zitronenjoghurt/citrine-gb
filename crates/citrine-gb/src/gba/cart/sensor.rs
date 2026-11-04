const GYRO_START: u8 = 1 << 0;
const GYRO_CLOCK: u8 = 1 << 1;
const GYRO_DATA: u8 = 1 << 2;
const GYRO_CENTRE: f32 = 1728.0;
const GYRO_ANTICLOCKWISE: f32 = 852.0;
const GYRO_CLOCKWISE: f32 = 2531.0;

const SOLAR_CLOCK: u8 = 1 << 0;
const SOLAR_RESET: u8 = 1 << 1;
const SOLAR_FLAG: u8 = 1 << 3;
const SOLAR_DARK: f32 = 232.0;
const SOLAR_SUN: f32 = 80.0;

pub(super) fn scale(value: f32, low: f32, centre: f32, high: f32) -> u16 {
    let value = if value.is_finite() {
        value.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    let span = if value < 0.0 {
        centre - low
    } else {
        high - centre
    };
    (centre + value * span).round() as u16
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Gyro {
    sample: u16,
    shift: u16,
    clock: bool,
}

impl Default for Gyro {
    fn default() -> Self {
        Self {
            sample: GYRO_CENTRE as u16,
            shift: 0,
            clock: false,
        }
    }
}

impl Gyro {
    pub fn set_turn(&mut self, rate: f32) {
        self.sample = scale(rate, GYRO_ANTICLOCKWISE, GYRO_CENTRE, GYRO_CLOCKWISE);
    }

    pub fn update(&mut self, pins: u8) {
        let clock = pins & GYRO_CLOCK != 0;
        if pins & GYRO_START != 0 {
            self.shift = self.sample & 0x0FFF;
        } else if self.clock && !clock {
            self.shift <<= 1;
        }
        self.clock = clock;
    }

    pub fn output(&self) -> u8 {
        if self.shift & 0x8000 != 0 {
            GYRO_DATA
        } else {
            0
        }
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Solar {
    level: u8,
    counter: u8,
    clock: bool,
}

impl Default for Solar {
    fn default() -> Self {
        Self {
            level: SOLAR_DARK as u8,
            counter: 0,
            clock: false,
        }
    }
}

impl Solar {
    pub fn set_sunlight(&mut self, light: f32) {
        let light = if light.is_finite() {
            light.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.level = (SOLAR_DARK + light * (SOLAR_SUN - SOLAR_DARK)).round() as u8;
    }

    pub fn update(&mut self, pins: u8) {
        let clock = pins & SOLAR_CLOCK != 0;
        if pins & SOLAR_RESET != 0 {
            self.counter = 0;
        } else if clock && !self.clock {
            self.counter = self.counter.wrapping_add(1);
        }
        self.clock = clock;
    }

    pub fn output(&self) -> u8 {
        if self.counter > self.level {
            SOLAR_FLAG
        } else {
            0
        }
    }
}

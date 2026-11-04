use super::Gba;
use crate::gba::cart::{DateTime, Devices};

impl Gba {
    pub fn cart_devices(&self) -> Devices {
        self.bus.cart().devices()
    }

    pub fn clock(&self) -> DateTime {
        self.bus.cart().clock()
    }

    pub fn set_clock(&mut self, local_unix: i64) {
        self.bus.cart_mut().set_clock(local_unix);
    }

    pub fn set_tilt(&mut self, x: f32, y: f32) {
        self.bus.cart_mut().set_tilt(x, y);
    }

    pub fn set_turn(&mut self, rate: f32) {
        self.bus.cart_mut().set_turn(rate);
    }

    pub fn set_sunlight(&mut self, light: f32) {
        self.bus.cart_mut().set_sunlight(light);
    }

    pub fn rumble(&self) -> bool {
        self.rumble
    }
}

use super::link::Link;
use super::registers::*;
use super::{IRQ_WAKE, Wifi};

pub const WAKE_US: u64 = 2048;
const ASLEEP: u16 = 1 << 9;
const WAKING: u16 = 1 << 8;
const WAKE_REQUEST: u16 = 1 << 1;
const SLEEP_REQUEST: u16 = 1 << 0;
const FORCE: u16 = 1 << 15;
const FORCE_OFF: u16 = 1 << 0;
const TRX_ON: u16 = 1;
const TRX_OFF: u16 = 2;
const POWER_STATE_MODE: u16 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Sleep {
    Forced,
    Auto,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Power {
    pub waking_until: Option<u64>,
    pub sleep_pending: Option<Sleep>,
}

impl Wifi {
    pub(super) fn awake(&self) -> bool {
        self.reg(W_POWERSTATE) & ASLEEP == 0
    }

    pub(super) fn request_wake(&mut self) {
        if self.reg(W_POWERFORCE) == FORCE | FORCE_OFF {
            return;
        }
        self.power.sleep_pending = None;
        if self.awake() || self.power.waking_until.is_some() {
            return;
        }
        let state = self.reg(W_POWERSTATE) | WAKING;
        self.set(W_POWERSTATE, state);
        self.power.waking_until = Some(self.now + WAKE_US);
        self.raise(IRQ_WAKE);
    }

    pub(super) fn check_wake(&mut self) -> bool {
        match self.power.waking_until {
            Some(at) if at <= self.now => {
                self.power.waking_until = None;
                self.set(W_POWERSTATE, 0);
                self.set(W_TRX_POWER, TRX_ON);
                self.set_rf_status(1);
                self.try_send();
                true
            }
            _ => false,
        }
    }

    pub(super) fn request_sleep(&mut self, how: Sleep) {
        self.power.waking_until = None;
        let state = self.reg(W_POWERSTATE) & !(WAKING | WAKE_REQUEST);
        self.set(W_POWERSTATE, state);
        if !self.awake() {
            return;
        }
        if self.link.in_flight() {
            self.power.sleep_pending = Some(how);
            self.set(W_TRX_POWER, TRX_OFF);
            return;
        }
        self.enter_sleep(how);
    }

    pub(super) fn frame_done(&mut self) {
        if let Some(how) = self.power.sleep_pending {
            self.enter_sleep(how);
        }
    }

    pub(super) fn write_power_force(&mut self, value: u16) {
        self.set(W_POWERFORCE, value);
        if value & FORCE != 0 {
            if value & FORCE_OFF != 0 {
                self.request_sleep(Sleep::Forced);
            } else {
                self.request_wake();
            }
        }
    }

    pub(super) fn write_power_state(&mut self, value: u16) {
        let state = (self.reg(W_POWERSTATE) & (ASLEEP | WAKING)) | (value & 3);
        self.set(W_POWERSTATE, state);
        if value & WAKE_REQUEST != 0 {
            if self.reg(W_MODE_RST) & 1 != 0 {
                self.request_wake();
            }
        } else if value & SLEEP_REQUEST != 0 && self.power_mode() == POWER_STATE_MODE {
            self.power_down();
        }
    }

    pub(super) fn post_beacon_sleep(&mut self) {
        if self.reg(W_POWER_TX) & 2 == 0 && self.power_mode() != POWER_STATE_MODE {
            self.power_down();
        }
    }

    fn power_mode(&self) -> u16 {
        self.reg(W_MODE_WEP) & 7
    }

    fn power_down(&mut self) {
        if self.reg(W_POWERDOWNCTRL) == 0 {
            self.request_sleep(Sleep::Auto);
        }
    }

    fn enter_sleep(&mut self, how: Sleep) {
        self.power.sleep_pending = None;
        self.set(W_POWERSTATE, ASLEEP);
        self.set(W_TRX_POWER, TRX_OFF);
        self.set_rf_status(9);
        if how == Sleep::Forced {
            self.set(W_TXREQ_READ, 0);
        }
        if !self.link.in_flight() {
            if matches!(
                self.link,
                Link::Listening | Link::Waiting(..) | Link::Failing
            ) {
                self.exchange = None;
            }
            self.link = Link::Idle;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{reg, wifi};
    use super::*;

    fn started() -> Wifi {
        let mut wifi = wifi();
        wifi.write16(W_POWER_US, 0);
        wifi.write16(W_MODE_RST, 1);
        wifi.write16(W_IE, 0xFFFF);
        wifi
    }

    #[test]
    fn the_radio_starts_asleep() {
        let mut wifi = started();
        assert_eq!(reg(&mut wifi, W_POWERSTATE), 0x0200);
        assert!(!wifi.awake());
    }

    #[test]
    fn a_wake_request_raises_irq11_and_finishes_after_2048_us() {
        let mut wifi = started();
        wifi.write16(W_POWERSTATE, 2);
        assert_eq!(reg(&mut wifi, W_IF), IRQ_WAKE);
        assert_eq!(reg(&mut wifi, W_POWERSTATE) & 0x0300, 0x0300);
        wifi.step_us(WAKE_US - 1);
        assert!(!wifi.awake());
        wifi.step_us(1);
        assert!(wifi.awake());
        assert_eq!(reg(&mut wifi, W_POWERSTATE), 0);
        assert_eq!(reg(&mut wifi, W_TRX_POWER), 1);
        assert_eq!(reg(&mut wifi, W_RF_STATUS), 1);
    }

    #[test]
    fn powerstate_wake_needs_mode_rst() {
        let mut wifi = wifi();
        wifi.write16(W_POWER_US, 0);
        wifi.write16(W_POWERSTATE, 2);
        wifi.step_us(5000);
        assert!(!wifi.awake());
    }

    #[test]
    fn powerforce_8001_enters_power_down_at_once_when_idle() {
        let mut wifi = started();
        wifi.write16(W_POWERFORCE, 0x8000);
        wifi.step_us(WAKE_US);
        assert!(wifi.awake());
        wifi.write16(W_TXREQ_SET, 0x0D);
        wifi.write16(W_POWERFORCE, 0x8001);
        assert_eq!(reg(&mut wifi, W_POWERSTATE), 0x0200);
        assert_eq!(reg(&mut wifi, W_TRX_POWER), 0x0002);
        assert_eq!(reg(&mut wifi, W_TXREQ_READ), 0);
        assert_eq!(reg(&mut wifi, W_RF_PINS), 0x0046);
        assert_eq!(reg(&mut wifi, W_RF_STATUS), 0x0009);
    }

    #[test]
    fn a_forced_power_down_holds_off_wake_requests() {
        let mut wifi = started();
        wifi.write16(W_POWERFORCE, 0x8001);
        wifi.write16(W_POWERSTATE, 2);
        wifi.step_us(WAKE_US * 2);
        assert!(!wifi.awake());
        wifi.write16(W_POWERFORCE, 0x8000);
        wifi.step_us(WAKE_US);
        assert!(wifi.awake());
    }

    fn woken() -> Wifi {
        let mut wifi = started();
        wifi.write16(W_POWERFORCE, 0x8000);
        wifi.write16(W_POWERFORCE, 0);
        wifi.step_us(WAKE_US);
        assert!(wifi.awake());
        wifi
    }

    #[test]
    fn a_powerstate_sleep_request_counts_only_in_mode_3() {
        let mut wifi = woken();
        wifi.write16(W_POWERSTATE, 1);
        wifi.step_us(10);
        assert!(wifi.awake());
        wifi.write16(W_MODE_WEP, 3);
        wifi.write16(W_POWERSTATE, 1);
        wifi.step_us(10);
        assert!(!wifi.awake());
    }

    #[test]
    fn post_beacon_stays_awake_in_mode_3() {
        let mut wifi = woken();
        wifi.write16(W_MODE_WEP, 3);
        wifi.write16(W_US_COUNTCNT, 1);
        wifi.write16(W_POWER_TX, 0);
        wifi.write16(W_POST_BEACON, 1);
        wifi.step_us(1024);
        assert!(wifi.awake());
    }

    #[test]
    fn powerdownctrl_holds_off_a_regular_power_down() {
        let mut wifi = woken();
        wifi.write16(W_POWERDOWNCTRL, 1);
        wifi.write16(W_US_COUNTCNT, 1);
        wifi.write16(W_POWER_TX, 0);
        wifi.write16(W_POST_BEACON, 1);
        wifi.step_us(1024);
        assert!(wifi.awake());
        wifi.write16(W_MODE_WEP, 3);
        wifi.write16(W_POWERSTATE, 1);
        wifi.step_us(10);
        assert!(wifi.awake());
    }

    #[test]
    fn post_beacon_sleeps_only_without_auto_sleep_disabled() {
        let mut wifi = started();
        wifi.write16(W_POWERFORCE, 0x8000);
        wifi.write16(W_POWERFORCE, 0);
        wifi.step_us(WAKE_US);
        wifi.write16(W_US_COUNTCNT, 1);
        wifi.write16(W_POST_BEACON, 1);
        wifi.step_us(1024);
        assert!(wifi.awake());
        wifi.write16(W_POWER_TX, 0);
        wifi.write16(W_POST_BEACON, 1);
        wifi.step_us(1024);
        assert!(!wifi.awake());
    }
}

use super::registers::*;
use super::{IRQ_BEACON, IRQ_POST_BEACON, IRQ_PRE_BEACON, Wifi};

pub const MS_US: u64 = 1024;
const MS_MASK: u64 = MS_US - 1;
const BEACON_BUSY: u16 = 1 << 4;
const DROPPED_REQUESTS: u16 = 0xFFF2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Compare,
    Count,
    Forced,
}

impl Wifi {
    pub(super) fn timer_event(&self) -> Option<u64> {
        self.counting().then(|| MS_US - (self.us_count & MS_MASK))
    }

    pub(super) fn pre_beacon_event(&self) -> Option<u64> {
        self.pre_beacon_at()
            .filter(|&at| at > self.us_count)
            .map(|at| at - self.us_count)
    }

    pub(super) fn check_pre_beacon(&mut self) -> bool {
        match self.pre_beacon_at() {
            Some(at) if at == self.us_count && self.pre_fired != Some(at) => {
                self.pre_fired = Some(at);
                self.pre_beacon();
                true
            }
            _ => false,
        }
    }

    pub(super) fn millisecond(&mut self) {
        let post = self.reg(W_POST_BEACON);
        if post != 0 {
            self.set(W_POST_BEACON, post - 1);
            if post == 1 {
                self.post_beacon();
            }
        }
        if self.reg(W_US_COMPARECNT) & 1 != 0 && self.us_count & !MS_MASK == self.compare() {
            self.block_beacon = false;
            self.beacon(Source::Compare);
        }
        let count = self.reg(W_BEACON_COUNT);
        if count != 0 {
            self.set(W_BEACON_COUNT, count - 1);
            if count == 1 {
                if self.reg(W_PRE_BEACON) == 0 {
                    self.pre_beacon();
                }
                self.beacon(Source::Count);
            }
        }
        if self.reg(W_BEACON_COUNT) == 0 {
            self.set(W_BEACON_COUNT, self.reg(W_BEACONINT));
        }
    }

    pub(super) fn beacon(&mut self, source: Source) {
        if source != Source::Forced {
            self.set(W_BEACON_COUNT, self.reg(W_BEACONINT));
        }
        if self.block_beacon && source == Source::Count {
            return;
        }
        if self.reg(W_US_COMPARECNT) & 1 == 0 {
            return;
        }
        self.raise(IRQ_BEACON);
        self.set(W_POST_BEACON, 0xFFFF);
        let requests = self.reg(W_TXREQ_READ) & DROPPED_REQUESTS;
        self.set(W_TXREQ_READ, requests);
        if self.reg(W_TXBUF_BEACON) & 0x8000 != 0 {
            let busy = self.reg(W_TXBUSY) | BEACON_BUSY;
            self.set(W_TXBUSY, busy);
        }
        let mut listen = self.reg(W_LISTENCOUNT);
        if listen == 0 {
            listen = self.reg(W_LISTENINT);
        }
        self.set(W_LISTENCOUNT, listen.saturating_sub(1));
        self.try_send();
    }

    fn counting(&self) -> bool {
        self.reg(W_US_COUNTCNT) & 1 != 0
    }

    fn compare(&self) -> u64 {
        (0..4).fold(0, |value, half| {
            value | (u64::from(self.reg(W_US_COMPARE0 + 2 * half)) << (16 * half))
        }) & !MS_MASK
    }

    fn pre_beacon_at(&self) -> Option<u64> {
        let pre = u64::from(self.reg(W_PRE_BEACON));
        let count = u64::from(self.reg(W_BEACON_COUNT));
        if !self.counting() || self.reg(W_US_COMPARECNT) & 1 == 0 || pre == 0 || count == 0 {
            return None;
        }
        ((self.us_count & !MS_MASK) + count * MS_US).checked_sub(pre)
    }

    fn pre_beacon(&mut self) {
        if !self.counting() || self.reg(W_US_COMPARECNT) & 1 == 0 {
            return;
        }
        self.raise(IRQ_PRE_BEACON);
        if self.reg(W_POWER_TX) & 1 != 0 {
            self.request_wake();
        }
    }

    fn post_beacon(&mut self) {
        self.raise(IRQ_POST_BEACON);
        self.post_beacon_sleep();
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{reg, wifi};
    use super::*;
    use crate::nds::wifi::IRQ_WAKE;

    fn started() -> Wifi {
        let mut wifi = wifi();
        wifi.write16(W_POWER_US, 0);
        wifi.write16(W_IE, 0xFFFF);
        wifi.write16(W_US_COUNTCNT, 1);
        wifi
    }

    fn first(wifi: &mut Wifi, bit: u16, limit: u64) -> Option<u64> {
        let start = wifi.now_us();
        while wifi.now_us() - start < limit {
            let wait = wifi.until_event_us().unwrap_or(limit).max(1);
            wifi.step_us(wait);
            if wifi.reg(W_IF) & bit != 0 {
                wifi.write16(W_IF, bit);
                return Some(wifi.now_us() - start);
            }
        }
        None
    }

    #[test]
    fn the_counter_counts_microseconds_only_when_enabled() {
        let mut wifi = wifi();
        wifi.write16(W_POWER_US, 0);
        wifi.step_us(500);
        assert_eq!(reg(&mut wifi, W_US_COUNT0), 0);
        wifi.write16(W_US_COUNTCNT, 1);
        wifi.step_us(500);
        assert_eq!(reg(&mut wifi, W_US_COUNT0), 500);
        wifi.write16(W_US_COUNT0 + 2, 0x0001);
        wifi.step_us(10);
        assert_eq!(wifi.read32(W_US_COUNT0), 0x0001_0000 + 510);
    }

    #[test]
    fn the_counter_runs_on_cpu_cycles_at_the_bus_clock() {
        let mut wifi = started();
        for _ in 0..1000 {
            wifi.step(33_514);
        }
        assert_eq!(wifi.read32(W_US_COUNT0), 1_000_000);
    }

    #[test]
    fn w_power_us_stops_the_whole_block() {
        let mut wifi = started();
        wifi.write16(W_POWER_US, 1);
        wifi.step_us(5000);
        assert_eq!(wifi.until_event_us(), None);
        assert_eq!(reg(&mut wifi, W_US_COUNT0), 0);
    }

    #[test]
    fn beacon_count_raises_irq14_every_beacon_interval_milliseconds() {
        let mut wifi = started();
        wifi.write16(W_US_COMPARECNT, 1);
        wifi.write16(W_BEACONINT, 3);
        wifi.write16(W_BEACON_COUNT, 3);
        assert_eq!(first(&mut wifi, IRQ_BEACON, 10_000), Some(3 * 1024));
        assert_eq!(reg(&mut wifi, W_BEACON_COUNT), 3);
        assert_eq!(first(&mut wifi, IRQ_BEACON, 10_000), Some(3 * 1024));
    }

    #[test]
    fn irq14_needs_the_compare_enable() {
        let mut wifi = started();
        wifi.write16(W_BEACONINT, 2);
        wifi.write16(W_BEACON_COUNT, 2);
        assert_eq!(first(&mut wifi, IRQ_BEACON, 6000), None);
    }

    #[test]
    fn the_compare_match_raises_irq14_at_its_millisecond() {
        let mut wifi = started();
        wifi.write16(W_US_COMPARECNT, 1);
        wifi.write16(W_BEACONINT, 0);
        wifi.write16(W_US_COMPARE0, 5 * 1024);
        for half in 1..4 {
            wifi.write16(W_US_COMPARE0 + 2 * half, 0);
        }
        assert_eq!(first(&mut wifi, IRQ_BEACON, 20_000), Some(5 * 1024));
    }

    #[test]
    fn compare_bit_zero_blocks_beacon_count_irqs_until_the_match() {
        let mut wifi = started();
        wifi.write16(W_US_COMPARECNT, 1);
        wifi.write16(W_BEACONINT, 1);
        wifi.write16(W_BEACON_COUNT, 1);
        wifi.write16(W_US_COMPARE0, (4 * 1024) | 1);
        for half in 1..4 {
            wifi.write16(W_US_COMPARE0 + 2 * half, 0);
        }
        assert_eq!(first(&mut wifi, IRQ_BEACON, 20_000), Some(4 * 1024));
        assert_eq!(first(&mut wifi, IRQ_BEACON, 20_000), Some(1024));
    }

    #[test]
    fn compare_cnt_bit_one_forces_irq14_without_a_reload() {
        let mut wifi = started();
        wifi.write16(W_BEACON_COUNT, 7);
        wifi.write16(W_US_COMPARECNT, 3);
        assert_eq!(reg(&mut wifi, W_IF) & IRQ_BEACON, IRQ_BEACON);
        assert_eq!(reg(&mut wifi, W_BEACON_COUNT), 7);
        assert_eq!(reg(&mut wifi, W_POST_BEACON), 0xFFFF);
        assert_eq!(reg(&mut wifi, W_US_COMPARECNT), 1);
    }

    #[test]
    fn irq15_comes_pre_beacon_microseconds_before_irq14() {
        let mut wifi = started();
        wifi.write16(W_US_COMPARECNT, 1);
        wifi.write16(W_BEACONINT, 4);
        wifi.write16(W_BEACON_COUNT, 4);
        wifi.write16(W_PRE_BEACON, 300);
        assert_eq!(
            first(&mut wifi, IRQ_PRE_BEACON, 10_000),
            Some(4 * 1024 - 300)
        );
        assert_eq!(first(&mut wifi, IRQ_BEACON, 10_000), Some(300));
    }

    #[test]
    fn irq15_with_no_pre_beacon_time_comes_with_irq14() {
        let mut wifi = started();
        wifi.write16(W_US_COMPARECNT, 1);
        wifi.write16(W_BEACONINT, 2);
        wifi.write16(W_BEACON_COUNT, 2);
        assert_eq!(first(&mut wifi, IRQ_BEACON, 10_000), Some(2048));
        assert_eq!(reg(&mut wifi, W_IF) & IRQ_PRE_BEACON, IRQ_PRE_BEACON);
    }

    #[test]
    fn post_beacon_counts_milliseconds_down_to_irq13_once() {
        let mut wifi = started();
        wifi.write16(W_POST_BEACON, 3);
        assert_eq!(first(&mut wifi, IRQ_POST_BEACON, 10_000), Some(3 * 1024));
        assert_eq!(reg(&mut wifi, W_POST_BEACON), 0);
        assert_eq!(first(&mut wifi, IRQ_POST_BEACON, 10_000), None);
    }

    #[test]
    fn each_beacon_reloads_post_beacon_and_steps_the_listen_count() {
        let mut wifi = started();
        wifi.write16(W_US_COMPARECNT, 1);
        wifi.write16(W_BEACONINT, 1);
        wifi.write16(W_BEACON_COUNT, 1);
        wifi.write16(W_LISTENINT, 4);
        let mut seen = Vec::new();
        for _ in 0..5 {
            first(&mut wifi, IRQ_BEACON, 2000);
            seen.push(reg(&mut wifi, W_LISTENCOUNT));
        }
        assert_eq!(seen, [3, 2, 1, 0, 3]);
        assert_eq!(reg(&mut wifi, W_POST_BEACON), 0xFFFF);
    }

    #[test]
    fn a_beacon_drops_the_loc_requests() {
        let mut wifi = started();
        wifi.write16(W_TXREQ_SET, 0xF);
        wifi.write16(W_US_COMPARECNT, 3);
        assert_eq!(reg(&mut wifi, W_TXREQ_READ), 0x12);
    }

    #[test]
    fn pre_beacon_wakes_the_radio_with_auto_wakeup() {
        let mut wifi = started();
        wifi.write16(W_POWER_TX, 1);
        wifi.write16(W_US_COMPARECNT, 1);
        wifi.write16(W_BEACONINT, 2);
        wifi.write16(W_BEACON_COUNT, 2);
        wifi.write16(W_PRE_BEACON, 100);
        assert_eq!(first(&mut wifi, IRQ_WAKE, 10_000), Some(2048 - 100));
    }

    #[test]
    fn cmd_count_counts_ten_microsecond_steps_down_to_zero() {
        let mut wifi = started();
        wifi.write16(W_CMD_COUNT, 100);
        wifi.step_us(255);
        assert_eq!(reg(&mut wifi, W_CMD_COUNT), 75);
        wifi.write16(W_CMD_COUNTCNT, 0);
        wifi.step_us(500);
        assert_eq!(reg(&mut wifi, W_CMD_COUNT), 75);
        wifi.write16(W_CMD_COUNTCNT, 1);
        wifi.step_us(5000);
        assert_eq!(reg(&mut wifi, W_CMD_COUNT), 0);
    }

    #[test]
    fn content_free_counts_down_even_without_the_counter() {
        let mut wifi = wifi();
        wifi.write16(W_POWER_US, 0);
        wifi.write16(W_CONTENTFREE, 1000);
        wifi.step_us(400);
        assert_eq!(reg(&mut wifi, W_CONTENTFREE), 600);
        wifi.step_us(4000);
        assert_eq!(reg(&mut wifi, W_CONTENTFREE), 0);
    }
}

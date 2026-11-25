use crate::Walker;
use crate::route::{self, LogKind};

const SECONDS_PER_MINUTE: u32 = 60;
const SECONDS_PER_HOUR: u32 = 3_600;
const HOURS_PER_DAY: u32 = 24;
const QUARTERS_PER_SECOND: u8 = 4;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Clock {
    last: Option<u64>,
    ms: u64,
    quarter: u8,
    held: bool,
}

impl Clock {
    pub(crate) fn restart(&mut self) {
        self.ms = 0;
        self.quarter = 0;
    }
}

impl Walker {
    pub(crate) fn release_clock(&mut self) {
        if std::mem::take(&mut self.clock.held) {
            self.seconds(1);
        }
    }
}

impl Walker {
    pub fn tick(&mut self, now: u64) {
        let last = *self.clock.last.get_or_insert(now);
        if now <= last {
            return;
        }
        self.clock.last = Some(now);
        let draw_ms = self.rules.draw_ms.max(1);
        let ms = self.clock.ms + (now - last);
        let quarters = ms / draw_ms;
        self.clock.ms = ms % draw_ms;
        let live = quarters.min(self.rules.catch_up_draws as u64);
        let mut skipped = quarters - live;
        while skipped > 0 {
            let to_second =
                u64::from((QUARTERS_PER_SECOND - self.clock.quarter) % QUARTERS_PER_SECOND);
            if to_second > 0 && skipped >= to_second {
                skipped -= to_second;
                self.clock.quarter = 0;
                self.seconds(1);
                continue;
            }
            if self.clock.quarter == 0 && skipped >= u64::from(QUARTERS_PER_SECOND) {
                let whole = skipped / u64::from(QUARTERS_PER_SECOND);
                skipped -= whole * u64::from(QUARTERS_PER_SECOND);
                self.seconds(whole);
                continue;
            }
            self.clock.quarter = (self.clock.quarter + skipped as u8) % QUARTERS_PER_SECOND;
            skipped = 0;
        }
        for _ in 0..live {
            self.quarter();
        }
    }

    pub fn ticked_at(&self) -> Option<u64> {
        self.clock.last
    }

    pub fn resume_at(&mut self, now: u64) {
        self.clock.last.get_or_insert(now);
    }

    pub fn asleep(&self) -> bool {
        self.ram.awake_seconds == 0
    }

    fn quarter(&mut self) {
        self.clock.quarter = (self.clock.quarter + 1) % QUARTERS_PER_SECOND;
        if self.link() != crate::Link::Idle {
            self.clock.held |= self.clock.quarter == 0;
            return;
        }
        if self.clock.quarter == 0 {
            self.seconds(1);
        }
        if self.asleep() {
            return;
        }
        for _ in 0..self.rules.passes_per_draw {
            self.pass();
        }
        self.draw();
    }

    fn seconds(&mut self, mut count: u64) {
        while count > 0 {
            let clock = self.clock();
            let to_hour = u64::from(SECONDS_PER_HOUR - clock % SECONDS_PER_HOUR);
            let step = count.min(to_hour);
            count -= step;
            self.run_seconds(step as u32);
        }
    }

    fn run_seconds(&mut self, seconds: u32) {
        let mut health = self.health();
        let before = health.clock();
        let after = before.wrapping_add(seconds);
        health.set_clock(after);
        let minutes = after / SECONDS_PER_MINUTE - before / SECONDS_PER_MINUTE;
        health.set_walk_minutes(
            health
                .walk_minutes()
                .saturating_add(minutes.min(u16::MAX as u32) as u16),
        );
        self.set_health(&health);
        self.ram.event_seconds = self
            .ram
            .event_seconds
            .saturating_add(seconds.min(u16::MAX as u32) as u16)
            .min(self.rules.home_event_seconds);
        self.ram.awake_seconds = u32::from(self.ram.awake_seconds).saturating_sub(seconds) as u8;
        if after.is_multiple_of(SECONDS_PER_HOUR) {
            self.hour();
        }
    }

    fn hour(&mut self) {
        let rules = &self.rules;
        let mut health = self.health();
        if health.hours() < rules.max_total_steps && health.total_steps() < rules.max_total_steps {
            health.set_hours(health.hours() + 1);
        }
        self.set_health(&health);
        self.save_health();
        if self.ram.walking {
            self.log(LogKind::Hour, 0, 0, None);
        }
        self.ram.hour_steps = 0;
        let hour = self.clock() / SECONDS_PER_HOUR % HOURS_PER_DAY;
        let rollover = match self.ram.rollover_hour {
            h if u32::from(h) < HOURS_PER_DAY => u32::from(h),
            _ => 0,
        };
        if hour == rollover {
            self.roll_day();
        }
    }

    fn roll_day(&mut self) {
        let rules = &self.rules;
        let mut health = self.health();
        health.set_total_days(health.total_days().saturating_add(1).min(rules.max_days));
        self.set_health(&health);
        self.save_health();
        let days = self.rules.history_days;
        route::push_history(&mut self.eeprom, days, self.ram.today_steps);
        self.ram.today_steps = 0;
        route::forget_met_peers(&mut self.eeprom, self.rules.met_peers);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eeprom::Block;
    use crate::info::FLAG_INIT;

    const HOUR_MS: u64 = 3_600_000;
    const DAY_MS: u64 = 24 * HOUR_MS;

    fn walker_at(clock: u32, rollover: u8) -> Walker {
        let mut walker = Walker::new(1);
        let mut health = walker.health();
        health.set_clock(clock);
        walker.set_health(&health);
        walker.ram.rollover_hour = rollover;
        let mut info = walker.info();
        info.set_flags(FLAG_INIT | (rollover << 3));
        walker.eeprom.write_reliable(Block::Info, info.bytes());
        walker.tick(0);
        walker
    }

    #[test]
    fn the_day_rolls_over_at_the_set_hour_into_the_history() {
        let midnight = 9_000 * 86_400;
        let mut walker = walker_at(midnight + 5 * 3_600, 6);
        walker.steps(1234);
        walker.tick(59 * 60_000);
        assert_eq!(walker.today_steps(), 1234);
        walker.tick(HOUR_MS);
        let health = walker.health();
        assert_eq!((walker.today_steps(), health.total_days()), (0, 1));
        assert_eq!(health.total_steps(), 1234);
        assert_eq!(walker.history(), vec![1234, 0, 0, 0, 0, 0, 0]);
        assert_eq!(health.clock(), midnight + 6 * 3_600);
        assert_eq!(health.hours(), 1);
    }

    #[test]
    fn days_away_each_roll_and_the_history_keeps_seven() {
        let mut walker = walker_at(0, 0);
        for day in 1..=3u64 {
            walker.steps(day as u32 * 100);
            walker.tick(day * DAY_MS);
        }
        assert_eq!(walker.history()[..3], [300, 200, 100]);
        walker.steps(50);
        walker.tick(13 * DAY_MS);
        let health = walker.health();
        assert_eq!(health.total_days(), 13);
        assert_eq!(health.hours(), 13 * 24);
        assert_eq!(walker.history(), vec![0; 7]);
    }

    #[test]
    fn a_restored_walker_resumed_at_its_last_tick_catches_up_the_days_between() {
        let walker = walker_at(0, 0);
        let restore = || Walker::restore(walker.eeprom.clone(), walker.memory());
        let mut resumed = restore();
        resumed.resume_at(0);
        resumed.steps(400);
        resumed.tick(2 * DAY_MS);
        assert_eq!(resumed.history()[..2], [0, 400]);
        assert_eq!(resumed.ticked_at(), Some(2 * DAY_MS));
        let mut fresh = restore();
        fresh.tick(2 * DAY_MS);
        assert_eq!(fresh.health().total_days(), 0);
    }

    #[test]
    fn the_rollover_clears_the_met_peers() {
        let mut walker = walker_at(0, 0);
        walker.eeprom.fill(0xDE24, 548, 0x11);
        walker.tick(DAY_MS);
        assert_eq!(walker.eeprom.read(0xDE2C, 0x28), vec![0xFF; 0x28]);
        assert_eq!(walker.eeprom.read(0xDE24, 8), vec![0x11; 8]);
    }

    #[test]
    fn the_screen_sleeps_after_its_awake_time_and_passes_stop() {
        let mut walker = walker_at(100, 0);
        let rng = walker.rng_state();
        walker.tick(1_000);
        assert_ne!(walker.rng_state(), rng);
        walker.tick(61_000);
        assert!(walker.asleep());
        assert_eq!(walker.screen(), crate::Screen::Home);
        let rng = walker.rng_state();
        walker.tick(70_000);
        assert_eq!(walker.rng_state(), rng);
        assert_eq!(walker.clock(), 170);
    }
}

use std::collections::VecDeque;
use std::time::Duration;
use web_time::Instant;

const KEPT: usize = 60;
const FITS_SLACK: f64 = 0.1;

#[derive(Debug, Default)]
pub(crate) struct ShowLag {
    needs: VecDeque<u32>,
}

impl ShowLag {
    pub(crate) fn finished(&mut self, took: Duration, first: Duration, refresh: Duration) {
        let over = took.saturating_sub(first).as_nanos();
        let need = over.div_ceil(refresh.as_nanos().max(1));
        if self.needs.len() == KEPT {
            self.needs.pop_front();
        }
        self.needs
            .push_back(u32::try_from(need).unwrap_or(u32::MAX));
    }

    pub(crate) fn lag(&self, refresh: Duration, frame: Duration) -> u32 {
        let fits = (frame.as_secs_f64() / refresh.as_secs_f64().max(1e-9) + FITS_SLACK).floor();
        let most = (fits as u32).saturating_sub(1);
        self.needs.iter().copied().max().unwrap_or(0).min(most)
    }
}

#[derive(Debug, Default)]
pub(crate) struct ShowPace {
    update: u64,
    slot: Option<u64>,
    lent_at: Option<(Instant, u64)>,
    delay: u64,
    lag: ShowLag,
}

impl ShowPace {
    pub(crate) fn begin(&mut self, refreshes: Option<u64>) {
        self.update += refreshes.unwrap_or(1);
    }

    pub(crate) fn lent(&mut self, now: Instant, frames: usize, lag: Option<(Duration, Duration)>) {
        let wait = match lag {
            Some((refresh, frame)) if frames > 0 => self.lag.lag(refresh, frame),
            _ => 0,
        };
        self.slot = Some(self.update + u64::from(wait));
        self.lent_at = (frames > 0).then_some((now, self.update));
    }

    pub(crate) fn refreshes_to_show(&self) -> Option<u64> {
        self.slot.map(|slot| slot.saturating_sub(self.update))
    }

    pub(crate) fn due(&self) -> bool {
        self.slot.is_none_or(|slot| self.update >= slot)
    }

    pub(crate) fn shown(
        &mut self,
        finished: Option<Instant>,
        first: Duration,
        refresh: Duration,
    ) -> bool {
        self.slot = None;
        let Some((lent_at, lent_update)) = self.lent_at.take() else {
            return false;
        };
        if let Some(finished) = finished {
            self.lag
                .finished(finished.saturating_duration_since(lent_at), first, refresh);
        }
        let delay = self.update - lent_update;
        let held = delay > self.delay;
        self.delay = delay;
        held
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REFRESH_180: Duration = Duration::from_nanos(5_555_556);
    const REFRESH_60: Duration = Duration::from_nanos(16_666_667);
    const FRAME: Duration = Duration::from_nanos(16_742_706);
    const TAIL: Duration = Duration::from_micros(1500);

    fn ms(ms: f64) -> Duration {
        Duration::from_secs_f64(ms / 1000.0)
    }

    #[test]
    fn a_frame_done_before_the_deadline_needs_no_lag() {
        let mut lag = ShowLag::default();
        lag.finished(ms(3.0), REFRESH_180 - TAIL, REFRESH_180);
        assert_eq!(lag.lag(REFRESH_180, FRAME), 0);
    }

    #[test]
    fn a_frame_past_one_deadline_needs_one_refresh() {
        let mut lag = ShowLag::default();
        lag.finished(ms(6.5), REFRESH_180 - TAIL, REFRESH_180);
        assert_eq!(lag.lag(REFRESH_180, FRAME), 1);
    }

    #[test]
    fn the_lag_never_reaches_the_next_console_frame() {
        let mut lag = ShowLag::default();
        lag.finished(ms(30.0), REFRESH_180 - TAIL, REFRESH_180);
        assert_eq!(lag.lag(REFRESH_180, FRAME), 2);
        assert_eq!(lag.lag(REFRESH_60, FRAME), 0);
    }

    #[test]
    fn a_refresh_measured_a_little_long_still_fits_three_in_a_ds_frame() {
        let ds_frame = Duration::from_nanos(16_715_000);
        let mut lag = ShowLag::default();
        lag.finished(ms(30.0), REFRESH_180 - TAIL, REFRESH_180);
        assert_eq!(lag.lag(ms(5.66), ds_frame), 2);
        assert_eq!(lag.lag(ms(6.06), ds_frame), 1);
    }

    #[test]
    fn one_slow_frame_holds_the_lag_for_a_second() {
        let mut lag = ShowLag::default();
        lag.finished(ms(10.0), REFRESH_180 - TAIL, REFRESH_180);
        for _ in 0..KEPT - 1 {
            lag.finished(ms(2.0), REFRESH_180 - TAIL, REFRESH_180);
        }
        assert_eq!(lag.lag(REFRESH_180, FRAME), 2);
        lag.finished(ms(2.0), REFRESH_180 - TAIL, REFRESH_180);
        assert_eq!(lag.lag(REFRESH_180, FRAME), 0);
    }

    struct Rng(u64);

    impl Rng {
        fn ms(&mut self, low: f64, high: f64) -> Duration {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let unit = (self.0 >> 11) as f64 / (1u64 << 53) as f64;
            ms(low + (high - low) * unit)
        }
    }

    struct Shown {
        updates: Vec<u64>,
        held: Vec<bool>,
    }

    fn run(refresh: Duration, every: u64, mut cost: impl FnMut(u64) -> Duration) -> Shown {
        let mut pace = ShowPace::default();
        let start = Instant::now();
        let mut away: Option<Instant> = None;
        let mut out = Shown {
            updates: Vec::new(),
            held: Vec::new(),
        };
        let mut show = |pace: &mut ShowPace, away: &mut Option<Instant>, update: u64| {
            out.held
                .push(pace.shown(away.take(), refresh - TAIL, refresh));
            out.updates.push(update);
        };
        for update in 0..600u64 {
            let begin = start + refresh * u32::try_from(update).expect("small");
            let deadline = begin + refresh - TAIL;
            pace.begin(None);
            if let Some(done) = away {
                if done > begin || !pace.due() {
                    if pace.due() && done <= deadline {
                        show(&mut pace, &mut away, update);
                    }
                    continue;
                }
                show(&mut pace, &mut away, update);
            }
            if update % every != 0 {
                continue;
            }
            pace.lent(begin, 1, Some((refresh, FRAME)));
            away = Some(begin + cost(update));
            if pace.due() && away.is_some_and(|done| done <= deadline) {
                show(&mut pace, &mut away, update);
            }
        }
        drop(show);
        out
    }

    fn gaps(shown: &Shown) -> Vec<u64> {
        shown
            .updates
            .windows(2)
            .skip(70)
            .map(|w| w[1] - w[0])
            .collect()
    }

    fn held_after_warmup(shown: &Shown) -> usize {
        shown.held.iter().skip(70).filter(|&&held| held).count()
    }

    #[test]
    fn at_180_hz_ds_costs_show_every_frame_for_three_refreshes() {
        let mut rng = Rng(7);
        let shown = run(REFRESH_180, 3, |_| rng.ms(4.0, 9.0));
        assert_eq!(held_after_warmup(&shown), 0);
        let gaps = gaps(&shown);
        assert!(gaps.len() > 100);
        assert!(gaps.iter().all(|&gap| gap == 3), "{gaps:?}");
    }

    #[test]
    fn at_180_hz_light_costs_show_every_frame_for_three_refreshes() {
        let mut rng = Rng(11);
        let shown = run(REFRESH_180, 3, |_| rng.ms(0.5, 3.0));
        assert!(gaps(&shown).iter().all(|&gap| gap == 3));
    }

    #[test]
    fn at_60_hz_a_frame_shows_in_its_own_update() {
        let mut rng = Rng(3);
        let shown = run(REFRESH_60, 1, |_| rng.ms(4.0, 9.0));
        assert!(gaps(&shown).iter().all(|&gap| gap == 1));
    }

    #[test]
    fn a_frame_shown_one_update_late_counts_as_held() {
        let shown = run(REFRESH_60, 1, |update| {
            if update == 300 { ms(20.0) } else { ms(3.0) }
        });
        assert_eq!(held_after_warmup(&shown), 1);
        let late = shown.updates.iter().position(|&update| update == 301);
        assert!(late.is_some_and(|at| shown.held[at]));
    }
}

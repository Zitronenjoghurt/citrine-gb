use super::pacer::FrameClock;
use citrine_gb::console::Timing;
use std::collections::VecDeque;
use std::time::Duration;

const WINDOW: usize = 32;
const TOLERANCE: f64 = 0.25;
const TOLERANCE_FLOOR_SECS: f64 = 0.003;
const SETTLED_TOLERANCE: f64 = 0.1;
const SETTLED_SHARE: f64 = 0.75;
const MIN_PERIOD_SECS: f64 = 1.0 / 250.0;
const MAX_PERIOD_SECS: f64 = 0.035;
const PERIOD_RESET: f64 = 0.1;
const PERIOD_FOLLOW: f64 = 1.0 / 256.0;
const DRIFT_GAIN: f64 = 0.05;
const MAX_DRIFT_SECS: f64 = 0.05;
const REFRESH_BAND: f64 = 0.75;
const PHASE_FOLLOW: f64 = 1.0 / 64.0;
const COUNT_SECONDS: usize = 10;
const MATCH_ENTER: f64 = 0.005;
const MATCH_LEAVE: f64 = 0.006;
const FIT_DECAY: f64 = 1.0 - 1.0 / 2048.0;
const FIT_LEAD: f64 = 16.0;
const FIT_READY: f64 = 48.0;
const FIT_RESET: f64 = 0.1;
const FIT_LOST: f64 = 0.25;
const FIT_SURE: f64 = 256.0;
const FIT_SURE_SHARE: f64 = 0.75;
const FIT_FASTER_UPDATES: u32 = 64;
const DUE_SLACK: f64 = 0.05;

#[derive(Debug, Default, Clone)]
pub struct RefreshSnap {
    dts: VecDeque<f64>,
    sorted: Vec<f64>,
    period: Option<f64>,
    drift: f64,
    phase: f64,
    last: Option<f64>,
    screen: Option<f64>,
    on_screen: bool,
}

impl RefreshSnap {
    pub fn snap(&mut self, dt: f64) -> f64 {
        let pair = self.last.map_or(dt, |last| (last + dt) / 2.0);
        self.last = Some(dt);
        if self.dts.len() == WINDOW {
            self.dts.pop_front();
        }
        self.dts.push_back(pair);

        let Some(median) = self.settled_median() else {
            self.period = None;
            self.on_screen = false;
            return self.release(dt);
        };
        let screen = self
            .screen
            .filter(|&screen| median >= screen * (1.0 - PERIOD_RESET));
        self.on_screen = screen.is_some();
        let period = match (screen, self.period) {
            (Some(screen), _) => {
                self.period = Some(screen);
                return self.on_refreshes(dt, screen);
            }
            (None, Some(period)) if (median - period).abs() <= period * PERIOD_RESET => {
                if near(pair, period) {
                    period + (pair - period) * PERIOD_FOLLOW
                } else {
                    period
                }
            }
            _ => {
                self.period = Some(self.mean_near(median));
                return self.release(dt);
            }
        };
        self.period = Some(period);

        let snapped = period + self.drift * DRIFT_GAIN;
        self.drift += dt - snapped;
        if self.drift.abs() > MAX_DRIFT_SECS {
            return self.release(snapped);
        }
        snapped
    }

    pub fn period(&self) -> Option<f64> {
        self.period.filter(|&period| period <= MAX_PERIOD_SECS)
    }

    pub fn set_screen_hz(&mut self, hz: Option<f64>) {
        self.screen = hz.filter(|&hz| hz > 0.0).map(|hz| 1.0 / hz);
    }

    fn screen_period(&self) -> Option<f64> {
        self.on_screen.then(|| self.period()).flatten()
    }

    fn on_refreshes(&mut self, dt: f64, period: f64) -> f64 {
        let ahead = self.drift + dt - self.phase;
        let refreshes = if (ahead - period).abs() < period * REFRESH_BAND {
            1.0
        } else {
            (ahead / period).round().max(0.0)
        };
        let snapped = refreshes * period;
        self.drift += dt - snapped;
        self.phase += (self.drift - self.phase) * PHASE_FOLLOW;
        if self.phase.abs() > period {
            self.phase -= (self.phase / period).round() * period;
        }
        snapped
    }

    fn release(&mut self, dt: f64) -> f64 {
        let out = (dt + self.drift).max(0.0);
        self.drift += dt - out;
        out
    }

    fn settled_median(&mut self) -> Option<f64> {
        if self.dts.len() < WINDOW {
            return None;
        }
        self.sorted.clear();
        self.sorted.extend(self.dts.iter().copied());
        self.sorted.sort_by(f64::total_cmp);
        let median = self.sorted[WINDOW / 2];
        let tolerance = settled_tolerance(median);
        let settled = self
            .dts
            .iter()
            .filter(|&&dt| near_by(dt, median, tolerance))
            .count();
        (median >= MIN_PERIOD_SECS && settled as f64 >= WINDOW as f64 * SETTLED_SHARE)
            .then_some(median)
    }

    fn mean_near(&self, median: f64) -> f64 {
        let tolerance = settled_tolerance(median);
        let (sum, count) = self
            .dts
            .iter()
            .filter(|&&dt| near_by(dt, median, tolerance))
            .fold((0.0, 0), |(sum, count), dt| (sum + dt, count + 1));
        sum / f64::from(count)
    }
}

fn settled_tolerance(median: f64) -> f64 {
    if median <= MAX_PERIOD_SECS {
        SETTLED_TOLERANCE
    } else {
        TOLERANCE
    }
}

fn near(dt: f64, period: f64) -> bool {
    near_by(dt, period, TOLERANCE)
}

fn near_by(dt: f64, period: f64, tolerance: f64) -> bool {
    (dt - period).abs() <= (period * tolerance).max(TOLERANCE_FLOOR_SECS)
}

#[derive(Debug, Default, Clone)]
pub struct PaceCount {
    shown: Option<f64>,
    secs: f64,
    repeated: usize,
    skipped: usize,
    excused: bool,
    seconds: VecDeque<(usize, usize)>,
    repeated_total: u64,
    skipped_total: u64,
}

impl PaceCount {
    pub fn record(&mut self, due: usize, dt: f64, frame_secs: f64, refresh: Option<f64>) {
        if let Some(shown) = &mut self.shown {
            *shown += dt.max(0.0);
        }
        if due > 0 {
            if let Some(shown) = self.shown.filter(|_| !self.excused) {
                let repeated = match refresh.filter(|&period| period > 0.0) {
                    Some(period) => {
                        let fits = (frame_secs / period - 0.05).ceil().max(1.0);
                        ((shown / period).round() - fits).max(0.0) as usize
                    }
                    None => ((shown / frame_secs).round() as usize).saturating_sub(due),
                };
                self.repeated += repeated;
                self.repeated_total += repeated as u64;
            }
            if !self.excused {
                self.skipped += due - 1;
                self.skipped_total += (due - 1) as u64;
            }
            self.excused = false;
            self.shown = Some(0.0);
        }

        self.secs += dt.max(0.0);
        if self.secs >= 1.0 {
            self.secs = (self.secs - 1.0).min(1.0);
            if self.seconds.len() == COUNT_SECONDS {
                self.seconds.pop_front();
            }
            self.seconds.push_back((self.repeated, self.skipped));
            self.repeated = 0;
            self.skipped = 0;
        }
    }

    #[cfg(any(test, all(feature = "nds", not(target_arch = "wasm32"))))]
    pub fn excuse(&mut self) {
        self.excused = true;
    }

    #[cfg(threaded)]
    pub fn held(&mut self) {
        self.repeated += 1;
        self.repeated_total += 1;
    }

    pub fn repeated_total(&self) -> u64 {
        self.repeated_total
    }

    pub fn skipped_total(&self) -> u64 {
        self.skipped_total
    }

    #[cfg(any(test, not(any(target_arch = "wasm32", target_os = "android"))))]
    pub fn repeated_per_sec(&self) -> f64 {
        self.per_sec(|(repeated, _)| repeated)
    }

    #[cfg(any(test, not(any(target_arch = "wasm32", target_os = "android"))))]
    pub fn skipped_per_sec(&self) -> f64 {
        self.per_sec(|(_, skipped)| skipped)
    }

    #[cfg(any(test, not(any(target_arch = "wasm32", target_os = "android"))))]
    fn per_sec(&self, pick: impl Fn((usize, usize)) -> usize) -> f64 {
        if self.seconds.is_empty() {
            return 0.0;
        }
        let total: usize = self.seconds.iter().map(|&second| pick(second)).sum();
        total as f64 / self.seconds.len() as f64
    }
}

#[derive(Debug, Default, Clone)]
pub struct EvenClock {
    snap: RefreshSnap,
    clock: FrameClock,
    count: PaceCount,
    fit: PeriodFit,
    trusted: Option<f64>,
    fit_from_trusted: bool,
    faster_for: u32,
    matching: bool,
    matched: Option<f64>,
}

impl EvenClock {
    pub fn refresh_period(&self) -> Option<f64> {
        self.snap.period()
    }

    pub fn set_matching(&mut self, matching: bool) {
        self.matching = matching;
    }

    pub fn set_screen_hz(&mut self, hz: Option<f64>) {
        self.snap.set_screen_hz(hz);
    }

    pub fn matching(&self) -> bool {
        self.matching
    }

    pub fn matched(&self) -> Option<f64> {
        self.matched
    }

    pub fn screen_matches(&self, timing: Timing) -> bool {
        matched_frame(timing.frame_secs(), self.snap.period(), true).is_some()
    }

    pub fn pace(&self, timing: Timing) -> f64 {
        self.matched
            .map_or(1.0, |frame| timing.frame_secs() / frame)
    }

    pub fn advance(&mut self, dt: f64, timing: Timing) -> usize {
        let even = self.snap.snap(dt);
        let period = self.snap.period();
        self.follow_fit(period);
        let start = if self.fit_from_trusted {
            self.trusted
        } else {
            period
        };
        if let Some(guess) = self.fit.period().or(start) {
            self.fit.push(dt, guess);
        }
        let fitted = self.snap.screen_period().or_else(|| self.fit.period());
        let matched = self
            .matching
            .then(|| matched_frame(timing.frame_secs(), fitted, self.matched.is_some()))
            .flatten();
        match (matched, self.matched, fitted) {
            (Some(_), None, Some(fitted)) => self.clock.center(fitted),
            (None, Some(_), _) => self.clock.uncenter(),
            _ => {}
        }
        self.matched = matched;
        let frame_secs = matched.unwrap_or_else(|| timing.frame_secs());
        let due = self.clock.advance_by(even, frame_secs);
        self.count.record(due, even, frame_secs, period);
        due
    }

    fn follow_fit(&mut self, period: Option<f64>) {
        let faster = matches!(
            (period, self.fit.period()),
            (Some(period), Some(fitted)) if period < fitted * (1.0 - FIT_RESET)
        );
        self.faster_for = if faster { self.faster_for + 1 } else { 0 };
        if self.faster_for >= FIT_FASTER_UPDATES {
            self.faster_for = 0;
            self.trusted = None;
            self.fit = PeriodFit::default();
        } else if self.fit.lost() {
            if self.fit_from_trusted {
                self.trusted = None;
            }
            self.fit = PeriodFit::default();
        } else if self.fit.sure() {
            self.trusted = self.fit.period();
        }
        if self.fit.is_empty() {
            self.fit_from_trusted = self.trusted.is_some();
        }
    }

    pub fn advance_over(&mut self, dt: f64, refreshes: u64, timing: Timing) -> usize {
        let part = dt / refreshes.max(1) as f64;
        (0..refreshes.max(1))
            .map(|_| self.advance(part, timing))
            .sum()
    }

    pub fn refreshes_to_due(&self, timing: Timing) -> Option<u64> {
        let period = self.snap.period()?;
        let frame_secs = self.matched.unwrap_or_else(|| timing.frame_secs());
        let left = self.clock.until_due(frame_secs) / period;
        Some((left - DUE_SLACK).ceil().max(1.0) as u64)
    }

    pub fn count(&self) -> &PaceCount {
        &self.count
    }

    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    pub fn excuse(&mut self) {
        self.count.excuse();
    }

    #[cfg(threaded)]
    pub fn held(&mut self) {
        self.count.held();
    }

    pub fn refresh(&self) -> Option<Duration> {
        self.snap.period().map(Duration::from_secs_f64)
    }

    #[cfg(test)]
    pub fn seed_refresh(&mut self, period: f64) {
        self.snap.period = Some(period);
    }
}

#[derive(Debug, Default, Clone)]
struct PeriodFit {
    weight: f64,
    mean_index: f64,
    mean_secs: f64,
    index_spread: f64,
    co_spread: f64,
    index: f64,
    secs: f64,
    miss_spread: f64,
    miss_weight: f64,
}

impl PeriodFit {
    fn push(&mut self, dt: f64, guess: f64) {
        self.secs += dt.max(0.0);
        let step = self.slope(FIT_LEAD).unwrap_or(guess);
        let index = if self.weight > 0.0 {
            (self.mean_index + (self.secs - self.mean_secs) / step).round()
        } else {
            0.0
        };
        if self.weight > 0.0 && index <= self.index {
            return;
        }
        if let Some(slope) = self.period() {
            let miss = self.secs - (self.mean_secs + slope * (index - self.mean_index));
            self.miss_spread = self.miss_spread * FIT_DECAY + miss * miss;
            self.miss_weight = self.miss_weight * FIT_DECAY + 1.0;
        }
        self.index = index;
        self.weight = self.weight * FIT_DECAY + 1.0;
        self.index_spread *= FIT_DECAY;
        self.co_spread *= FIT_DECAY;
        let share = 1.0 / self.weight;
        let index_gap = index - self.mean_index;
        self.mean_index += index_gap * share;
        self.mean_secs += (self.secs - self.mean_secs) * share;
        self.index_spread += index_gap * (index - self.mean_index);
        self.co_spread += index_gap * (self.secs - self.mean_secs);
    }

    fn slope(&self, weight: f64) -> Option<f64> {
        (self.weight >= weight && self.index_spread > 0.0)
            .then(|| self.co_spread / self.index_spread)
    }

    fn period(&self) -> Option<f64> {
        self.slope(FIT_READY)
    }

    fn is_empty(&self) -> bool {
        self.weight == 0.0
    }

    fn miss(&self) -> Option<f64> {
        (self.miss_weight >= FIT_READY).then(|| (self.miss_spread / self.miss_weight).sqrt())
    }

    fn lost(&self) -> bool {
        matches!((self.period(), self.miss()), (Some(period), Some(miss)) if miss > period * FIT_LOST)
    }

    fn sure(&self) -> bool {
        self.weight >= FIT_SURE
            && matches!(
                (self.period(), self.miss()),
                (Some(period), Some(miss)) if miss <= period * FIT_LOST * FIT_SURE_SHARE
            )
    }
}

fn matched_frame(frame_secs: f64, period: Option<f64>, held: bool) -> Option<f64> {
    let period = period.filter(|&period| period > 0.0)?;
    let refreshes = (frame_secs / period).round().max(1.0);
    let within = if held { MATCH_LEAVE } else { MATCH_ENTER };
    let matched = refreshes * period;
    (0.0..=within)
        .contains(&(frame_secs / matched - 1.0))
        .then_some(matched)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: Timing = Timing {
        clock_hz: 4_194_304,
        cycles_per_frame: 70_224,
    };

    struct Stamps {
        seed: u64,
    }

    impl Stamps {
        fn new(seed: u64) -> Self {
            Self { seed }
        }

        fn uniform(&mut self) -> f64 {
            self.seed ^= self.seed << 13;
            self.seed ^= self.seed >> 7;
            self.seed ^= self.seed << 17;
            ((self.seed >> 11) as f64 + 0.5) / (1u64 << 53) as f64
        }

        fn gauss(&mut self, sigma: f64) -> f64 {
            let (a, b) = (self.uniform(), self.uniform());
            (-2.0 * a.ln()).sqrt() * (std::f64::consts::TAU * b).cos() * sigma
        }

        fn vsyncs(&mut self, stretches: &[(f64, f64)], jitter_secs: f64) -> Vec<f64> {
            let mut start = 0.0;
            let mut stamps = Vec::new();
            for &(hz, secs) in stretches {
                for i in 0..(hz * secs) as usize {
                    stamps.push(start + i as f64 / hz + self.gauss(jitter_secs));
                }
                start += secs;
            }
            let mut last = stamps[0];
            stamps
                .into_iter()
                .map(|now| {
                    let dt = now - last;
                    last = now;
                    dt
                })
                .collect()
        }
    }

    struct Run {
        holds: Vec<(f64, usize, usize)>,
        clock: EvenClock,
    }

    impl Run {
        fn even(dts: &[f64]) -> Self {
            let mut clock = EvenClock::default();
            let dues: Vec<usize> = dts.iter().map(|&dt| clock.advance(dt, GB)).collect();
            Self::from_dues(dts, &dues, clock)
        }

        fn raw(dts: &[f64]) -> Self {
            let mut frames = FrameClock::default();
            let mut clock = EvenClock::default();
            let dues: Vec<usize> = dts
                .iter()
                .map(|&dt| {
                    let due = frames.advance(dt, GB);
                    clock.count.record(due, dt, GB.frame_secs(), None);
                    due
                })
                .collect();
            Self::from_dues(dts, &dues, clock)
        }

        fn from_dues(dts: &[f64], dues: &[usize], clock: EvenClock) -> Self {
            let mut now = 0.0;
            let mut held = None;
            let mut holds = Vec::new();
            for (&dt, &due) in dts.iter().zip(dues) {
                now += dt;
                if due == 0 {
                    if let Some(held) = &mut held {
                        *held += 1;
                    }
                    continue;
                }
                if let Some(held) = held {
                    holds.push((now, held, due));
                }
                held = Some(1);
            }
            Self { holds, clock }
        }

        fn repeated_per_sec(&self, hz: f64, from: f64, to: f64) -> f64 {
            let even = (GB.frame_secs() * hz).round() as usize;
            let repeated: usize = self
                .holds
                .iter()
                .filter(|&&(at, _, _)| (from..to).contains(&at))
                .map(|&(_, held, _)| held.saturating_sub(even))
                .sum();
            repeated as f64 / (to - from)
        }

        fn skipped_per_sec(&self, from: f64, to: f64) -> f64 {
            let skipped: usize = self
                .holds
                .iter()
                .filter(|&&(at, _, _)| (from..to).contains(&at))
                .map(|&(_, _, due)| due - 1)
                .sum();
            skipped as f64 / (to - from)
        }

        fn uneven_per_sec(&self, hz: f64, from: f64, to: f64) -> f64 {
            let even = (GB.frame_secs() * hz).round() as usize;
            let uneven = self
                .holds
                .iter()
                .filter(|&&(at, held, due)| (from..to).contains(&at) && (held != even || due > 1))
                .count();
            uneven as f64 / (to - from)
        }
    }

    #[test]
    fn jitter_at_60_hz_leaves_only_the_beat_of_the_game_speed() {
        let dts = Stamps::new(1).vsyncs(&[(60.0, 120.0)], 0.001);
        let raw = Run::raw(&dts);
        let even = Run::even(&dts);
        assert!(raw.repeated_per_sec(60.0, 1.0, 120.0) > 1.5);
        assert!(raw.skipped_per_sec(1.0, 120.0) > 1.2);
        let repeated = even.repeated_per_sec(60.0, 1.0, 120.0);
        assert!((0.2..=0.4).contains(&repeated), "repeated {repeated}");
        assert!(even.skipped_per_sec(1.0, 120.0) < 0.05);
        assert!(even.clock.count().repeated_per_sec() < 1.0);
        assert_eq!(even.clock.count().skipped_per_sec(), 0.0);
    }

    #[test]
    fn jitter_at_120_hz_keeps_frames_even() {
        let dts = Stamps::new(2).vsyncs(&[(120.0, 120.0)], 0.001);
        let raw = Run::raw(&dts).uneven_per_sec(120.0, 1.0, 120.0);
        let even = Run::even(&dts);
        let uneven = even.uneven_per_sec(120.0, 1.0, 120.0);
        assert!(raw > 6.0, "raw {raw}");
        assert!(uneven < 0.8, "uneven {uneven}");
        let repeated = even.repeated_per_sec(120.0, 1.0, 120.0);
        assert!(repeated < 0.8, "repeated {repeated}");
    }

    #[test]
    fn late_updates_at_180_hz_keep_frames_even() {
        let mut stamps = Stamps::new(4);
        let hz = 180.0;
        let mut last = 0.0;
        let dts: Vec<f64> = (1..(hz * 60.0) as usize)
            .map(|i| {
                let late = if stamps.uniform() < 0.3 { 0.003 } else { 0.0 };
                let now = i as f64 / hz + stamps.gauss(0.0005) + late;
                let dt = now - last;
                last = now;
                dt
            })
            .collect();
        let even = Run::even(&dts);
        let uneven = even.uneven_per_sec(hz, 1.0, 60.0);
        assert!(uneven < 3.0, "uneven {uneven}");
        let period = even.clock.refresh_period().expect("settled");
        assert!((period * hz - 1.0).abs() < 0.01, "period {period}");
    }

    #[test]
    fn a_refresh_change_settles_within_a_second() {
        let mut stamps = Stamps::new(3);
        let up = Run::even(&stamps.vsyncs(&[(60.0, 10.0), (120.0, 20.0)], 0.001));
        let uneven = up.uneven_per_sec(120.0, 11.0, 30.0);
        assert!(uneven < 0.8, "60 to 120 Hz: uneven {uneven}");

        let down = Run::even(&stamps.vsyncs(&[(120.0, 10.0), (60.0, 20.0)], 0.001));
        let uneven = down.uneven_per_sec(60.0, 11.0, 30.0);
        assert!(uneven < 0.5, "120 to 60 Hz: uneven {uneven}");
    }

    #[test]
    fn game_time_keeps_up_with_the_wall_clock() {
        for (hz, seed) in [(60.0, 4), (120.0, 5), (144.0, 6)] {
            let dts = Stamps::new(seed).vsyncs(&[(hz, 120.0)], 0.001);
            let mut snap = RefreshSnap::default();
            let mut wall = 0.0;
            let mut game = 0.0;
            for &dt in &dts {
                wall += dt;
                game += snap.snap(dt);
                assert!((wall - game).abs() <= MAX_DRIFT_SECS + 0.01, "{hz} Hz");
            }
        }
    }

    #[test]
    fn unsettled_timing_passes_through() {
        let mut stamps = Stamps::new(7);
        let mut snap = RefreshSnap::default();
        for _ in 0..600 {
            let dt = stamps.uniform() * 0.04;
            assert_eq!(snap.snap(dt), dt);
        }
        let mut snap = RefreshSnap::default();
        for _ in 0..600 {
            let dt = 0.001 + stamps.uniform() * 0.0005;
            assert_eq!(snap.snap(dt), dt);
        }
        assert_eq!(snap.period(), None);
    }

    #[test]
    fn updates_slower_than_a_refresh_are_no_refresh() {
        let mut snap = RefreshSnap::default();
        for _ in 0..200 {
            assert!((snap.snap(0.0704) - 0.0704).abs() < 1e-9);
        }
        assert_eq!(snap.period(), None);
    }

    #[test]
    fn a_stall_is_paid_back_at_once() {
        let mut snap = RefreshSnap::default();
        for _ in 0..100 {
            snap.snap(1.0 / 60.0);
        }
        let out = snap.snap(0.2);
        assert!((out - 0.2).abs() < 0.01, "out {out}");
        assert!(snap.period().is_some());
    }

    #[cfg(threaded)]
    #[test]
    fn a_held_console_frame_counts_as_repeated() {
        let mut clock = EvenClock::default();
        for _ in 0..120 {
            clock.advance(1.0 / 60.0, GB);
        }
        let even = clock.count().repeated_total();
        clock.held();
        assert_eq!(clock.count().repeated_total(), even + 1);
    }

    #[test]
    fn count_reads_per_second() {
        let mut count = PaceCount::default();
        for frame in 0..600 {
            let due = match frame % 60 {
                0 => 0,
                30 => 2,
                _ => 1,
            };
            count.record(due, 1.0 / 60.0, GB.frame_secs(), None);
        }
        assert!((count.repeated_per_sec() - 1.0).abs() < 0.2);
        assert!((count.skipped_per_sec() - 1.0).abs() < 0.2);
    }

    #[test]
    fn an_excused_frame_counts_no_repeat_or_skip() {
        let mut count = PaceCount::default();
        for frame in 0..600 {
            let due = match frame % 60 {
                29 | 59 => 0,
                30 => 2,
                _ => 1,
            };
            count.record(due, 1.0 / 60.0, GB.frame_secs(), None);
            if frame % 30 == 28 {
                count.excuse();
            }
        }
        assert_eq!(count.repeated_per_sec(), 0.0);
        assert_eq!(count.skipped_per_sec(), 0.0);
    }

    #[test]
    fn a_frame_held_while_the_clock_catches_up_counts_as_repeated() {
        let mut count = PaceCount::default();
        let refresh = Some(1.0 / 60.0);
        for due in [1, 1, 0, 2, 1, 1] {
            count.record(due, 1.0 / 60.0, GB.frame_secs(), refresh);
        }
        assert_eq!(count.repeated_total(), 1);
        assert_eq!(count.skipped_total(), 1);
    }

    #[test]
    fn at_180_hz_a_fourth_refresh_counts_as_repeated() {
        let mut count = PaceCount::default();
        let refresh = 1.0 / 180.0;
        for due in [1, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1] {
            count.record(due, refresh, GB.frame_secs(), Some(refresh));
        }
        assert_eq!(count.repeated_total(), 1);
    }

    #[test]
    fn at_144_hz_the_two_three_cadence_counts_no_repeats() {
        let mut count = PaceCount::default();
        let refresh = 1.0 / 144.0;
        for due in [1, 0, 1, 0, 0, 1, 0, 1, 0, 0, 1, 0, 1] {
            count.record(due, refresh, GB.frame_secs(), Some(refresh));
        }
        assert_eq!(count.repeated_total(), 0);
    }

    #[test]
    fn an_even_game_counts_only_the_beat_at_any_refresh() {
        for (hz, seed) in [(120.0, 5), (144.0, 7), (165.0, 8), (180.0, 6), (240.0, 9)] {
            let fps = 1.0 / GB.frame_secs();
            let fits = (hz / fps - 0.05_f64).ceil();
            let beat = (hz - fits * fps).max(0.0);
            let dts = Stamps::new(seed).vsyncs(&[(hz, 30.0)], 0.0005);
            let mut clock = EvenClock::default();
            for &dt in &dts {
                clock.advance(dt, GB);
            }
            let repeated = clock.count().repeated_per_sec();
            assert!(
                repeated <= beat + 0.25,
                "{hz} Hz: {repeated} repeats a second, beat {beat}"
            );
        }
    }

    #[test]
    fn extra_updates_between_refreshes_are_not_repeats() {
        let dts = Stamps::new(10).vsyncs(&[(240.0, 30.0)], 0.0005);
        let mut clock = EvenClock::default();
        for &dt in &dts {
            for part in [0.0005, dt - 0.0005] {
                clock.advance(part, GB);
            }
        }
        let repeated = clock.count().repeated_per_sec();
        assert!(repeated < 1.0, "{repeated} repeats a second");
    }

    fn matched_run(dts: &[f64], timing: Timing) -> (Vec<usize>, EvenClock) {
        let mut clock = EvenClock::default();
        clock.set_matching(true);
        let dues = dts.iter().map(|&dt| clock.advance(dt, timing)).collect();
        (dues, clock)
    }

    #[test]
    fn matching_the_screen_shows_each_frame_equally_long() {
        for hz in [60.0, 120.0, 180.0, 240.0] {
            let jitter = if hz > 120.0 { 0.0005 } else { 0.001 };
            let dts = Stamps::new(hz as u64).vsyncs(&[(hz, 60.0)], jitter);
            let (dues, mut clock) = matched_run(&dts, GB);
            let every = (hz / 60.0) as usize;
            let settled = &dues[(hz * 2.0) as usize..];
            assert!(
                settled.iter().all(|&due| due <= 1),
                "{hz} Hz ran two at once"
            );
            let shown: Vec<usize> = settled
                .iter()
                .enumerate()
                .filter(|&(_, &due)| due > 0)
                .map(|(at, _)| at)
                .collect();
            assert!(
                shown.windows(2).all(|pair| pair[1] - pair[0] == every),
                "{hz} Hz uneven"
            );
            assert_eq!(clock.count().repeated_per_sec(), 0.0, "{hz} Hz repeats");
            let pace = clock.pace(GB);
            assert!(
                (pace - 60.0 * GB.frame_secs()).abs() < 1e-4,
                "{hz} Hz pace {pace}"
            );
            clock.set_matching(false);
            clock.advance(1.0 / hz, GB);
            assert_eq!(clock.pace(GB), 1.0);
        }
    }

    #[test]
    fn matching_leaves_144_and_165_hz_at_the_console_speed() {
        for hz in [144.0, 165.0] {
            let dts = Stamps::new(7).vsyncs(&[(hz, 10.0)], 0.001);
            let (_, clock) = matched_run(&dts, GB);
            assert_eq!(clock.pace(GB), 1.0, "{hz} Hz");
        }
    }

    fn late_updates(seed: u64, hz: f64, secs: f64) -> Vec<f64> {
        let mut stamps = Stamps::new(seed);
        let mut last = 0.0;
        (1..(hz * secs) as usize)
            .map(|i| {
                let late = if stamps.uniform() < 0.3 { 0.003 } else { 0.0 };
                let now = i as f64 / hz + stamps.gauss(0.0005) + late;
                let dt = now - last;
                last = now;
                dt
            })
            .collect()
    }

    #[test]
    fn the_screen_rate_from_the_system_keeps_late_updates_matched() {
        let gba = citrine_gb::console::ConsoleKind::Gba.timing();
        let hz = 180.0;
        for seed in 1..6 {
            let mut clock = EvenClock::default();
            clock.set_matching(true);
            clock.set_screen_hz(Some(hz));
            let dues: Vec<(usize, bool)> = late_updates(seed * 7919, hz, 60.0)
                .into_iter()
                .map(|dt| (clock.advance(dt, gba), clock.matched().is_some()))
                .collect();
            let settled = &dues[(hz * 2.0) as usize..];
            assert!(settled.iter().all(|&(_, matched)| matched), "seed {seed}");
            let shown: Vec<usize> = settled
                .iter()
                .enumerate()
                .filter(|&(_, &(due, _))| due > 0)
                .map(|(at, _)| at)
                .collect();
            let uneven = shown
                .windows(2)
                .filter(|pair| pair[1] - pair[0] != 3)
                .count();
            assert!(uneven <= 12, "seed {seed}: {uneven} uneven frames");
        }
    }

    #[test]
    fn a_screen_rate_the_updates_disagree_with_is_left_out() {
        let dts = Stamps::new(11).vsyncs(&[(120.0, 10.0)], 0.001);
        let mut clock = EvenClock::default();
        clock.set_matching(true);
        clock.set_screen_hz(Some(60.0));
        for dt in dts {
            clock.advance(dt, GB);
        }
        let period = clock.refresh_period().expect("settled");
        assert!((period * 120.0 - 1.0).abs() < 0.01, "period {period}");
        assert!(clock.matched().is_some());
    }

    #[test]
    fn a_ds_on_a_60_hz_screen_runs_three_tenths_of_a_percent_faster() {
        let ds = citrine_gb::console::ConsoleKind::Nds.timing();
        let dts = Stamps::new(9).vsyncs(&[(60.0, 10.0)], 0.001);
        let (_, clock) = matched_run(&dts, ds);
        let pace = clock.pace(ds);
        assert!((pace - 1.00291).abs() < 2e-4, "pace {pace}");
    }

    #[test]
    fn only_a_screen_up_to_half_a_percent_faster_matches() {
        let frame = GB.frame_secs();
        assert!(matched_frame(frame, Some(1.0 / 60.0), false).is_some());
        assert!(matched_frame(frame, Some(1.0 / 119.88), false).is_some());
        assert_eq!(matched_frame(frame, Some(1.0 / 59.5), false), None);
        assert_eq!(matched_frame(frame, Some(1.0 / 60.1), false), None);
        assert_eq!(matched_frame(frame, Some(1.0 / 144.0), false), None);
        assert!(matched_frame(frame, Some(1.0 / 60.05), true).is_some());
    }

    #[test]
    fn a_clock_left_off_runs_at_the_console_speed() {
        let dts = Stamps::new(3).vsyncs(&[(60.0, 5.0)], 0.001);
        let mut clock = EvenClock::default();
        for dt in dts {
            clock.advance(dt, GB);
        }
        assert_eq!(clock.pace(GB), 1.0);
    }

    struct Skipping {
        frames_at: Vec<usize>,
        passes: usize,
    }

    fn skipping_run(dts: &[f64], timing: Timing, matching: bool) -> Skipping {
        let mut clock = EvenClock::default();
        clock.set_matching(matching);
        let mut frames_at = Vec::new();
        let mut passes = 0;
        let mut wake_at = 0;
        let mut last = 0;
        let mut owed = 0.0;
        for (at, &dt) in dts.iter().enumerate() {
            owed += dt;
            if at < wake_at {
                continue;
            }
            passes += 1;
            let refreshes = (at - last).max(1) as u64;
            last = at;
            if clock.advance_over(owed, refreshes, timing) > 0 {
                frames_at.push(at);
            }
            owed = 0.0;
            wake_at = at + clock.refreshes_to_due(timing).unwrap_or(1) as usize;
        }
        Skipping { frames_at, passes }
    }

    fn every_refresh_run(dts: &[f64], timing: Timing, matching: bool) -> Vec<usize> {
        let mut clock = EvenClock::default();
        clock.set_matching(matching);
        dts.iter()
            .enumerate()
            .filter(|&(_, &dt)| clock.advance(dt, timing) > 0)
            .map(|(at, _)| at)
            .collect()
    }

    fn intervals(frames_at: &[usize], from: usize) -> Vec<usize> {
        let settled: Vec<usize> = frames_at.iter().copied().filter(|&at| at > from).collect();
        settled.windows(2).map(|pair| pair[1] - pair[0]).collect()
    }

    #[test]
    fn skipping_refreshes_without_a_game_frame_runs_the_same_frames() {
        for hz in [120.0, 180.0, 240.0] {
            for matching in [true, false] {
                let dts = Stamps::new(hz as u64 + 5).vsyncs(&[(hz, 60.0)], 0.0005);
                let every = every_refresh_run(&dts, GB, matching);
                let skipping = skipping_run(&dts, GB, matching);
                let settled = (hz * 2.0) as usize;
                let shown_every = intervals(&every, settled);
                let shown_skipping = intervals(&skipping.frames_at, settled);
                let differ: usize = (1..8)
                    .map(|gap| {
                        let count = |shown: &[usize]| shown.iter().filter(|&&g| g == gap).count();
                        count(&shown_every).abs_diff(count(&shown_skipping))
                    })
                    .sum();
                assert!(
                    differ <= every.len() / 200,
                    "{hz} Hz matching {matching}: {differ} of {} intervals differ",
                    every.len()
                );
                if matching {
                    let moved = every
                        .iter()
                        .filter(|&&at| at > settled && !skipping.frames_at.contains(&at))
                        .count();
                    assert!(
                        moved <= every.len() / 200,
                        "{hz} Hz: {moved} of {} frames moved",
                        every.len()
                    );
                }
                let per_frame = skipping.passes as f64 / skipping.frames_at.len() as f64;
                let most = if matching { 1.1 } else { 1.2 };
                assert!(
                    per_frame <= most,
                    "{hz} Hz matching {matching}: {per_frame:.2} passes a frame"
                );
            }
        }
    }

    #[test]
    fn the_due_count_waits_for_the_refresh_before_the_frame() {
        let dts = Stamps::new(4).vsyncs(&[(120.0, 5.0)], 0.0005);
        let mut clock = EvenClock::default();
        clock.set_matching(true);
        let mut ran = false;
        for &dt in &dts {
            ran = clock.advance(dt, GB) > 0;
        }
        let due = clock.refreshes_to_due(GB).expect("a refresh period");
        assert!((1..=2).contains(&due), "{due} after a frame ran {ran}");
    }
}

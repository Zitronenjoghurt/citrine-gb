use crate::audio::even_clock::RefreshSnap;
use std::time::Duration;
use web_time::Instant;

const RELIST: Duration = Duration::from_secs(2);
const OUTPACED: f64 = 0.15;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateSource {
    WindowsDisplayConfig,
    WindowsDisplaySettings,
    MacDisplayMode,
    MacScreenMaximum,
}

impl RateSource {
    pub fn name(self) -> &'static str {
        match self {
            Self::WindowsDisplayConfig => "windows_display_config",
            Self::WindowsDisplaySettings => "windows_display_settings",
            Self::MacDisplayMode => "mac_display_mode",
            Self::MacScreenMaximum => "mac_screen_maximum",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub min: [f64; 2],
    pub max: [f64; 2],
}

impl Rect {
    fn overlap(&self, other: &Rect) -> f64 {
        let side = |axis: usize| {
            (self.max[axis].min(other.max[axis]) - self.min[axis].max(other.min[axis])).max(0.0)
        };
        side(0) * side(1)
    }

    fn distance_to(&self, [x, y]: [f64; 2]) -> f64 {
        let dx = (self.min[0] - x).max(x - self.max[0]).max(0.0);
        let dy = (self.min[1] - y).max(y - self.max[1]).max(0.0);
        dx.hypot(dy)
    }

    fn center(&self) -> [f64; 2] {
        [
            (self.min[0] + self.max[0]) / 2.0,
            (self.min[1] + self.max[1]) / 2.0,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonitorRate {
    pub hz: f64,
    pub source: RateSource,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Monitor {
    pub id: u64,
    pub rect: Rect,
    pub rate: Option<MonitorRate>,
}

pub trait Screens {
    fn window(&mut self) -> Option<Rect>;
    fn monitors(&mut self) -> Vec<Monitor>;
}

pub fn pick(window: Rect, monitors: &[Monitor]) -> Option<&Monitor> {
    let most = monitors
        .iter()
        .map(|monitor| (monitor, monitor.rect.overlap(&window)))
        .filter(|(_, overlap)| *overlap > 0.0)
        .max_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(monitor, _)| monitor);
    most.or_else(|| {
        let center = window.center();
        monitors.iter().min_by(|a, b| {
            a.rect
                .distance_to(center)
                .total_cmp(&b.rect.distance_to(center))
        })
    })
}

#[derive(Debug, Default)]
pub struct WindowMonitor {
    monitors: Vec<Monitor>,
    listed_at: Option<Instant>,
    on: Option<u64>,
}

impl WindowMonitor {
    pub fn follow(&mut self, screens: &mut impl Screens, now: Instant) {
        let stale = self
            .listed_at
            .is_none_or(|at| now.saturating_duration_since(at) >= RELIST);
        if stale {
            self.monitors = screens.monitors();
            self.listed_at = Some(now);
        }
        self.on = screens
            .window()
            .and_then(|window| pick(window, &self.monitors))
            .map(|monitor| monitor.id);
    }

    pub fn monitor(&self) -> Option<&Monitor> {
        let on = self.on?;
        self.monitors.iter().find(|monitor| monitor.id == on)
    }

    pub fn rate(&self) -> Option<MonitorRate> {
        self.monitor()?.rate
    }
}

#[derive(Debug, Default)]
pub struct PresentPace {
    snap: RefreshSnap,
    last_start: Option<Instant>,
    hz: Option<f64>,
    outpaced: bool,
    next: Option<Instant>,
}

impl PresentPace {
    pub fn plan(
        &mut self,
        started: Instant,
        monitor_hz: Option<f64>,
        now: Instant,
    ) -> Option<Instant> {
        if let Some(last) = self.last_start.replace(started) {
            self.snap
                .snap(started.saturating_duration_since(last).as_secs_f64());
        }
        if monitor_hz != self.hz {
            self.hz = monitor_hz;
            self.outpaced = false;
            self.next = None;
        }
        let period = 1.0 / monitor_hz.filter(|hz| *hz > 0.0)?;
        if let Some(measured) = self.snap.period()
            && measured < period * (1.0 - OUTPACED)
        {
            self.outpaced = true;
        }
        if !self.outpaced {
            return None;
        }
        let period = Duration::from_secs_f64(period);
        let next = match self.next {
            Some(next) if now < next => next,
            Some(next) if now < next + period => next + period,
            _ => now + period,
        };
        self.next = Some(next);
        Some(next)
    }

    pub fn paced(&self) -> bool {
        self.outpaced
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::even_clock::EvenClock;
    use citrine_gb::console::ConsoleKind;

    const WIDE: Rect = Rect {
        min: [0.0, 0.0],
        max: [2560.0, 1440.0],
    };
    const SIDE: Rect = Rect {
        min: [2560.0, 0.0],
        max: [3840.0, 720.0],
    };

    fn monitor(id: u64, rect: Rect, hz: f64) -> Monitor {
        Monitor {
            id,
            rect,
            rate: Some(MonitorRate {
                hz,
                source: RateSource::WindowsDisplayConfig,
            }),
        }
    }

    fn two_screens() -> Vec<Monitor> {
        vec![monitor(1, WIDE, 240.0), monitor(2, SIDE, 60.0)]
    }

    fn window(x: f64, y: f64) -> Rect {
        Rect {
            min: [x, y],
            max: [x + 1280.0, y + 673.0],
        }
    }

    struct Mock {
        window: Option<Rect>,
        monitors: Vec<Monitor>,
        listings: usize,
    }

    impl Screens for Mock {
        fn window(&mut self) -> Option<Rect> {
            self.window
        }

        fn monitors(&mut self) -> Vec<Monitor> {
            self.listings += 1;
            self.monitors.clone()
        }
    }

    #[test]
    fn the_window_is_on_the_monitor_holding_most_of_it() {
        let monitors = two_screens();
        assert_eq!(pick(window(2560.0, 0.0), &monitors).unwrap().id, 2);
        assert_eq!(pick(window(100.0, 100.0), &monitors).unwrap().id, 1);
        assert_eq!(pick(window(2000.0, 0.0), &monitors).unwrap().id, 2);
        assert_eq!(pick(window(1500.0, 0.0), &monitors).unwrap().id, 1);
    }

    #[test]
    fn a_window_off_every_monitor_takes_the_nearest() {
        let monitors = two_screens();
        assert_eq!(pick(window(4500.0, 100.0), &monitors).unwrap().id, 2);
        assert_eq!(pick(window(-2000.0, 100.0), &monitors).unwrap().id, 1);
        assert_eq!(pick(window(0.0, 0.0), &[]), None);
    }

    #[test]
    fn the_rate_follows_the_window_to_another_monitor() {
        let mut screens = Mock {
            window: Some(window(100.0, 100.0)),
            monitors: two_screens(),
            listings: 0,
        };
        let mut follow = WindowMonitor::default();
        let start = Instant::now();
        follow.follow(&mut screens, start);
        assert_eq!(follow.rate().unwrap().hz, 240.0);

        screens.window = Some(window(2600.0, 20.0));
        follow.follow(&mut screens, start + Duration::from_millis(16));
        assert_eq!(follow.rate().unwrap().hz, 60.0);
        assert_eq!(screens.listings, 1, "a move needs no new listing");
    }

    #[test]
    fn a_changed_mode_shows_within_the_relist_time() {
        let mut screens = Mock {
            window: Some(window(2600.0, 20.0)),
            monitors: two_screens(),
            listings: 0,
        };
        let mut follow = WindowMonitor::default();
        let start = Instant::now();
        follow.follow(&mut screens, start);
        screens.monitors[1] = monitor(2, SIDE, 144.0);
        follow.follow(&mut screens, start + Duration::from_secs(1));
        assert_eq!(follow.rate().unwrap().hz, 60.0);
        follow.follow(&mut screens, start + RELIST);
        assert_eq!(follow.rate().unwrap().hz, 144.0);
    }

    #[test]
    fn no_monitor_list_or_no_rate_gives_no_rate() {
        let mut screens = Mock {
            window: Some(window(0.0, 0.0)),
            monitors: Vec::new(),
            listings: 0,
        };
        let mut follow = WindowMonitor::default();
        follow.follow(&mut screens, Instant::now());
        assert_eq!(follow.rate(), None);

        let mut screens = Mock {
            window: Some(window(0.0, 0.0)),
            monitors: vec![Monitor {
                id: 1,
                rect: WIDE,
                rate: None,
            }],
            listings: 0,
        };
        follow = WindowMonitor::default();
        follow.follow(&mut screens, Instant::now());
        assert_eq!(follow.monitor().unwrap().id, 1);
        assert_eq!(follow.rate(), None);
    }

    struct Run {
        updates: usize,
        clock: EvenClock,
        paced: bool,
    }

    fn run_on(screens: &mut Mock, vsync_hz: f64, secs: f64) -> Run {
        let work = 0.001;
        let vblank = |at: f64| (at * vsync_hz).ceil() / vsync_hz;
        let start = Instant::now();
        let at = |secs: f64| start + Duration::from_secs_f64(secs);
        let mut follow = WindowMonitor::default();
        let mut pace = PresentPace::default();
        let mut clock = EvenClock::default();
        clock.set_matching(true);
        let mut t = 0.0;
        let mut last = None;
        let mut updates = 0;
        while t < secs {
            if t >= secs / 2.0 {
                updates += 1;
            }
            if let Some(last) = last {
                clock.advance(t - last, ConsoleKind::Gb.timing());
            }
            last = Some(t);
            follow.follow(screens, at(t));
            let done = t + work;
            let next = pace.plan(at(t), follow.rate().map(|rate| rate.hz), at(done));
            let presented = vblank(done);
            t = match next {
                Some(next) => next.duration_since(start).as_secs_f64().max(presented),
                None => presented,
            };
        }
        Run {
            updates,
            clock,
            paced: pace.paced(),
        }
    }

    #[test]
    fn a_window_on_the_60_hz_monitor_updates_60_times_a_second_under_a_240_hz_vsync() {
        let mut screens = Mock {
            window: Some(window(2600.0, 20.0)),
            monitors: two_screens(),
            listings: 0,
        };
        let run = run_on(&mut screens, 240.0, 10.0);
        assert!(run.paced);
        assert!((295..=305).contains(&run.updates), "{}", run.updates);
        let period = run.clock.refresh_period().unwrap();
        assert!((period * 60.0 - 1.0).abs() < 0.01, "{period}");
        assert!(run.clock.screen_matches(ConsoleKind::Gb.timing()));
    }

    #[test]
    fn a_window_on_the_240_hz_monitor_keeps_every_vsync() {
        let mut screens = Mock {
            window: Some(window(100.0, 100.0)),
            monitors: two_screens(),
            listings: 0,
        };
        let run = run_on(&mut screens, 240.0, 10.0);
        assert!(!run.paced);
        assert!((1195..=1205).contains(&run.updates), "{}", run.updates);
    }

    #[test]
    fn a_vsync_at_the_monitors_own_rate_is_never_paced() {
        let mut screens = Mock {
            window: Some(window(2600.0, 20.0)),
            monitors: two_screens(),
            listings: 0,
        };
        let run = run_on(&mut screens, 60.0, 10.0);
        assert!(!run.paced);
        assert!((299..=301).contains(&run.updates), "{}", run.updates);
    }

    #[test]
    fn moving_back_to_the_fast_monitor_stops_the_pacing() {
        let start = Instant::now();
        let mut pace = PresentPace::default();
        let mut t = Duration::ZERO;
        let step = Duration::from_secs_f64(1.0 / 240.0);
        for _ in 0..100 {
            pace.plan(start + t, Some(60.0), start + t);
            t += step;
        }
        assert!(pace.paced());
        assert_eq!(pace.plan(start + t, Some(240.0), start + t), None);
        assert!(!pace.paced());
    }

    #[test]
    fn an_early_extra_update_keeps_the_next_deadline() {
        let start = Instant::now();
        let mut pace = PresentPace::default();
        let mut t = Duration::ZERO;
        let step = Duration::from_secs_f64(1.0 / 240.0);
        let mut next = None;
        while !pace.paced() {
            next = pace.plan(start + t, Some(60.0), start + t);
            t += step;
        }
        let next = next.unwrap();
        let early = start + t;
        assert!(early < next);
        assert_eq!(pace.plan(early, Some(60.0), early), Some(next));
    }
}

use super::even_clock::{EvenClock, RefreshSnap};
use citrine_gb::console::{ConsoleKind, Timing};

const EMERALD: &[u8] = include_bytes!("../../fixtures/frame_times/emerald.dt");
const MKDS_RACE: &[u8] = include_bytes!("../../fixtures/frame_times/mkds-race.dt");
const MKDS_TAPING: &[u8] = include_bytes!("../../fixtures/frame_times/mkds-taping.dt");
const FINAL_FANTASY_ADVENTURE: &[u8] =
    include_bytes!("../../fixtures/frame_times/final-fantasy-adventure.dt");
const LOOPS: usize = 10;
const SCREEN_HZ: f64 = 180.0;
const SEED_AT: usize = 40;
const EMERALD_SEED: f64 = 0.005_582;
const MKDS_RACE_SEED: f64 = 0.006_773;

fn update_times(bytes: &[u8]) -> Vec<f64> {
    let (pairs, _) = bytes.as_chunks::<2>();
    pairs
        .iter()
        .map(|&pair| f64::from(u16::from_le_bytes(pair)) / 1e6)
        .collect()
}

struct Replay {
    at: Vec<f64>,
    matched: Vec<bool>,
    repeated: u64,
}

impl Replay {
    fn run(dts: &[f64], timing: Timing, screen_hz: Option<f64>, seed: f64) -> Self {
        let mut clock = EvenClock::default();
        clock.set_matching(true);
        clock.set_screen_hz(screen_hz);
        let mut now = 0.0;
        let mut replay = Self {
            at: Vec::with_capacity(dts.len()),
            matched: Vec::with_capacity(dts.len()),
            repeated: 0,
        };
        for (update, &dt) in dts.iter().enumerate() {
            clock.advance(dt, timing);
            if update == SEED_AT {
                clock.seed_refresh(seed);
            }
            now += dt;
            replay.at.push(now);
            replay.matched.push(clock.matched().is_some());
        }
        replay.repeated = clock.count().repeated_total();
        replay
    }

    fn from(&self, secs: f64) -> &[bool] {
        let start = self.at.partition_point(|&at| at < secs);
        &self.matched[start..]
    }

    fn matched_share(&self, secs: f64) -> f64 {
        let matched = self.from(secs);
        matched.iter().filter(|&&on| on).count() as f64 / matched.len() as f64
    }

    fn switches(&self, secs: f64) -> usize {
        self.from(secs)
            .windows(2)
            .filter(|pair| pair[0] != pair[1])
            .count()
    }

    fn repeated_per_minute(&self) -> f64 {
        self.repeated as f64 * 60.0 / self.at.last().copied().unwrap_or(1.0)
    }
}

fn worst_lag(bytes: &[u8]) -> f64 {
    let dts = update_times(bytes);
    let mut snap = RefreshSnap::default();
    snap.set_screen_hz(Some(SCREEN_HZ));
    let (mut wall, mut game, mut worst) = (0.0, 0.0, 0.0_f64);
    for &dt in dts.iter().cycle().take(dts.len() * LOOPS) {
        wall += dt;
        game += snap.snap(dt);
        worst = worst.max((wall - game).abs());
    }
    worst
}

fn emerald(screen_hz: Option<f64>) -> Replay {
    Replay::run(
        &update_times(EMERALD),
        ConsoleKind::Gba.timing(),
        screen_hz,
        EMERALD_SEED,
    )
}

fn mkds_race(screen_hz: Option<f64>) -> Replay {
    Replay::run(
        &update_times(MKDS_RACE),
        ConsoleKind::Nds.timing(),
        screen_hz,
        MKDS_RACE_SEED,
    )
}

#[test]
fn emerald_matches_again_after_the_ui_change() {
    let replay = emerald(None);
    assert_eq!(replay.matched_share(10.0), 1.0);
    assert!(
        replay.matched_share(1.0) > 0.98,
        "{}",
        replay.matched_share(1.0)
    );
}

#[test]
fn emerald_with_the_screen_rate_stays_matched_through_the_ui_change() {
    let replay = emerald(Some(SCREEN_HZ));
    assert_eq!(replay.matched_share(1.0), 1.0);
    assert!(
        replay.repeated_per_minute() < 20.0,
        "{}",
        replay.repeated_per_minute()
    );
}

#[test]
fn a_gpu_bound_race_stays_matched_with_the_screen_rate() {
    let replay = mkds_race(Some(SCREEN_HZ));
    assert_eq!(replay.matched_share(1.0), 1.0);
    assert_eq!(replay.switches(1.0), 0);
}

#[test]
fn a_gpu_bound_race_keeps_the_refresh_learned_before_it() {
    let replay = mkds_race(None);
    assert!(
        replay.matched_share(5.0) > 0.99,
        "{}",
        replay.matched_share(5.0)
    );
    assert!(replay.switches(5.0) <= 2, "{}", replay.switches(5.0));
}

#[test]
fn a_race_while_taping_keeps_game_time_within_two_refreshes() {
    let lag = worst_lag(MKDS_TAPING);
    assert!(lag <= 2.0 / SCREEN_HZ, "{lag}");
}

#[test]
fn a_long_game_boy_session_keeps_game_time_within_two_refreshes() {
    let lag = worst_lag(FINAL_FANTASY_ADVENTURE);
    assert!(lag <= 2.0 / SCREEN_HZ, "{lag}");
}

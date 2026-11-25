use crate::storage::{CrashSave, SaveStore, now_unix};
use citrine_gb::console::ConsoleKind;
use citrine_gb::persistence::battery_save::BatterySave;
use citrine_gb::persistence::state::StateBody;
use citrine_gb_threads::{Claim, Publisher, Reader, latest};
use std::fmt::Display;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use strum_macros::EnumIter;

pub const DEFAULT_KEEP: usize = 10;
pub const MAX_KEEP: usize = 50;
pub fn max_keep(kind: ConsoleKind) -> usize {
    match kind {
        ConsoleKind::Gb | ConsoleKind::Gbc | ConsoleKind::Gba | ConsoleKind::Nds => MAX_KEEP,
    }
}

pub fn keep_for(kind: ConsoleKind, keep: usize) -> usize {
    keep.clamp(1, max_keep(kind))
}

const LAST_GOOD_QUALITY: u32 = 6;
const COPY_EVERY: Duration = Duration::from_secs(1);

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum AutosaveInterval {
    Off,
    OneMinute,
    TwoMinutes,
    #[default]
    FiveMinutes,
    TenMinutes,
    FifteenMinutes,
    ThirtyMinutes,
}

impl AutosaveInterval {
    pub fn minutes(self) -> Option<u64> {
        match self {
            Self::Off => None,
            Self::OneMinute => Some(1),
            Self::TwoMinutes => Some(2),
            Self::FiveMinutes => Some(5),
            Self::TenMinutes => Some(10),
            Self::FifteenMinutes => Some(15),
            Self::ThirtyMinutes => Some(30),
        }
    }

    pub fn period(self) -> Option<Duration> {
        self.minutes().map(|m| Duration::from_secs(m * 60))
    }
}

impl Display for AutosaveInterval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.minutes() {
            None => f.write_str("Off"),
            Some(minutes) => write!(f, "{minutes} min"),
        }
    }
}

#[derive(Debug, Default)]
pub struct PlayClock {
    since_autosave: Duration,
    since_copy: Duration,
    unsaved: bool,
}

impl PlayClock {
    pub fn play(&mut self, dt: Duration) {
        self.since_autosave += dt;
        self.since_copy += dt;
        self.unsaved = true;
    }

    pub fn autosave_due(&self, interval: AutosaveInterval) -> bool {
        interval
            .period()
            .is_some_and(|period| self.since_autosave >= period)
    }

    pub fn copy_due(&self, paused: bool) -> bool {
        if paused {
            self.since_copy > Duration::ZERO
        } else {
            self.since_copy >= COPY_EVERY
        }
    }

    pub fn copied(&mut self) {
        self.since_copy = Duration::ZERO;
    }

    pub fn attempted(&mut self) {
        self.since_autosave = Duration::ZERO;
    }

    pub fn saved(&mut self) {
        self.since_autosave = Duration::ZERO;
        self.unsaved = false;
    }

    pub fn has_unsaved_play(&self) -> bool {
        self.unsaved
    }

    pub fn copy_written(&mut self) {
        if self.since_copy == Duration::ZERO {
            self.saved();
        }
    }
}

pub struct StateCopy {
    pub store: SaveStore,
    pub rom_key: String,
    pub state: StateBody,
    pub frame: Vec<u8>,
    pub battery: Option<BatterySave>,
    pub keep: usize,
    pub unsaved: bool,
}

pub struct LastGood {
    publisher: Publisher<Option<StateCopy>>,
    crash: CrashCopy,
}

#[derive(Clone)]
pub struct CrashCopy(Arc<Shared>);

struct Shared {
    reader: Claim<Reader<Option<StateCopy>>>,
    unsaved: AtomicBool,
    written: AtomicBool,
}

impl Default for LastGood {
    fn default() -> Self {
        let (publisher, reader) = latest();
        Self {
            publisher,
            crash: CrashCopy(Arc::new(Shared {
                reader: Claim::new(reader),
                unsaved: AtomicBool::new(false),
                written: AtomicBool::new(false),
            })),
        }
    }
}

impl LastGood {
    pub fn keep(&mut self, copy: StateCopy) {
        let unsaved = copy.unsaved;
        self.publisher.publish(|slot| *slot = Some(copy));
        self.crash.0.unsaved.store(unsaved, Ordering::Release);
    }

    pub fn clear(&mut self) {
        self.crash.0.unsaved.store(false, Ordering::Release);
        self.publisher.publish(|slot| *slot = None);
    }

    pub fn take_written(&self) -> bool {
        self.crash.0.written.swap(false, Ordering::AcqRel)
    }

    pub fn mark_saved(&self) {
        self.crash.0.unsaved.store(false, Ordering::Release);
    }

    pub fn crash_copy(&self) -> CrashCopy {
        self.crash.clone()
    }

    pub fn write_out(&self) -> Result<Option<usize>, String> {
        self.crash.write_out()
    }
}

impl CrashCopy {
    pub fn write_out(&self) -> Result<Option<usize>, String> {
        let shared = &self.0;
        shared
            .reader
            .try_with(|reader| {
                let Some(copy) = reader.newest() else {
                    return Ok(None);
                };
                if !shared.unsaved.swap(false, Ordering::AcqRel) {
                    return Ok(None);
                }
                let written = write_copy(copy);
                match &written {
                    Ok(_) => shared.written.store(true, Ordering::Release),
                    Err(_) => shared.unsaved.store(true, Ordering::Release),
                }
                written.map(Some)
            })
            .unwrap_or_else(|| Err("the last state is in use".into()))
    }
}

fn write_copy(copy: &StateCopy) -> Result<usize, String> {
    let state = match copy
        .state
        .pack(copy.state.quality(false).min(LAST_GOOD_QUALITY))
    {
        Ok(state) => state,
        Err(err) => {
            if let Some(save) = &copy.battery {
                let _ = copy.store.store_battery_save(&copy.rom_key, save);
            }
            return Err(err.to_string());
        }
    };
    let crash = CrashSave::new(
        copy.rom_key.clone(),
        now_unix(),
        copy.state.kind,
        state,
        &copy.frame,
        copy.battery.as_ref(),
    );
    let stored = copy
        .store
        .store_crash_save(&crash, keep_for(copy.state.kind, copy.keep));
    #[cfg(target_arch = "wasm32")]
    crate::storage::keep_crash_save(&crash);
    stored.map_err(|err| err.to_string())
}

pub fn install_panic_hook(last_good: CrashCopy) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        match last_good.write_out() {
            Ok(Some(id)) => log::error!("autosaved the last good state as autosave {id}"),
            Ok(None) => {}
            Err(err) => log::error!("could not autosave after the crash: {err}"),
        }
    }));
}

#[cfg(target_arch = "wasm32")]
pub fn save_when_hidden(last_good: CrashCopy) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;

    let Some(window) = web_sys::window() else {
        return;
    };
    let write_out = move |only_if_hidden: bool| {
        let hidden = web_sys::window()
            .and_then(|w| w.document())
            .is_none_or(|d| d.hidden());
        if only_if_hidden && !hidden {
            return;
        }
        if let Err(err) = last_good.write_out() {
            log::error!("could not autosave when the page was hidden: {err}");
        }
    };
    let on_hide = write_out.clone();
    let hidden = Closure::<dyn FnMut()>::new(move || on_hide(true));
    let leaving = Closure::<dyn FnMut()>::new(move || write_out(false));
    if let Some(document) = window.document() {
        let _ = document
            .add_event_listener_with_callback("visibilitychange", hidden.as_ref().unchecked_ref());
    }
    let _ = window.add_event_listener_with_callback("pagehide", leaving.as_ref().unchecked_ref());
    hidden.forget();
    leaving.forget();
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    const KEY: &str = "GAME";

    fn store(name: &str) -> (SaveStore, std::path::PathBuf) {
        let root = crate::storage::scratch_dir().join(format!(
            "citrine-autosave-test-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        (SaveStore::at(root.clone()), root)
    }

    fn copy(store: &SaveStore, unsaved: bool) -> StateCopy {
        StateCopy {
            store: store.clone(),
            rom_key: KEY.into(),
            state: StateBody {
                kind: ConsoleKind::Gb,
                data: b"state".to_vec(),
            },
            frame: vec![0; 160 * 144 * 4],
            battery: Some(BatterySave::raw(b"sram".to_vec())),
            keep: 3,
            unsaved,
        }
    }

    #[test]
    fn only_play_time_counts() {
        let mut clock = PlayClock::default();
        let interval = AutosaveInterval::OneMinute;
        clock.play(Duration::from_secs(59));
        assert!(!clock.autosave_due(interval));
        clock.play(Duration::from_secs(1));
        assert!(clock.autosave_due(interval));
        clock.saved();
        assert!(!clock.autosave_due(interval));
        assert!(!clock.has_unsaved_play());
    }

    #[test]
    fn off_never_comes_due() {
        let mut clock = PlayClock::default();
        clock.play(Duration::from_secs(3 * 3600));
        assert!(!clock.autosave_due(AutosaveInterval::Off));
        assert!(clock.has_unsaved_play());
    }

    #[test]
    fn a_failed_attempt_waits_a_full_interval() {
        let mut clock = PlayClock::default();
        clock.play(Duration::from_secs(120));
        clock.attempted();
        assert!(!clock.autosave_due(AutosaveInterval::OneMinute));
        assert!(clock.has_unsaved_play(), "nothing was saved yet");
    }

    #[test]
    fn the_copy_follows_play_and_pause() {
        let mut clock = PlayClock::default();
        assert!(!clock.copy_due(true), "nothing played since the last copy");
        clock.play(Duration::from_millis(300));
        assert!(!clock.copy_due(false));
        assert!(clock.copy_due(true), "pausing takes a fresh copy");
        clock.play(Duration::from_millis(700));
        assert!(clock.copy_due(false));
        clock.copied();
        assert!(!clock.copy_due(true));
    }

    #[test]
    fn intervals_read_as_minutes() {
        assert_eq!(AutosaveInterval::Off.to_string(), "Off");
        assert_eq!(AutosaveInterval::FiveMinutes.to_string(), "5 min");
        assert_eq!(AutosaveInterval::default(), AutosaveInterval::FiveMinutes);
    }

    #[test]
    fn writing_out_saves_once_and_flushes_the_battery() {
        let (store, root) = store("write-out");
        let mut last_good = LastGood::default();
        assert_eq!(last_good.write_out(), Ok(None));

        last_good.keep(copy(&store, true));
        assert_eq!(last_good.write_out(), Ok(Some(0)));
        assert_eq!(last_good.write_out(), Ok(None), "nothing new to save");
        assert_eq!(store.load_battery(KEY).as_deref(), Some(&b"sram"[..]));

        let saved = store.load_autosave(KEY, 0).expect("autosave");
        let mut back = Vec::new();
        std::io::Read::read_to_end(&mut brotli::Decompressor::new(&saved[..], 4096), &mut back)
            .expect("brotli");
        assert_eq!(back, b"state");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_saved_copy_is_not_written_again() {
        let (store, root) = store("marked");
        let mut last_good = LastGood::default();
        last_good.keep(copy(&store, true));
        last_good.mark_saved();
        assert_eq!(last_good.write_out(), Ok(None));
        assert!(store.autosave_ids(KEY).is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_held_lock_is_skipped_not_waited_on() {
        let (store, root) = store("locked");
        let mut last_good = LastGood::default();
        last_good.keep(copy(&store, true));
        let inside = last_good.crash.0.reader.try_with(|_| last_good.write_out());
        assert!(matches!(inside, Some(Err(_))));
        assert_eq!(last_good.write_out(), Ok(Some(0)));
        assert!(last_good.take_written());
        assert!(!last_good.take_written());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_panic_writes_an_autosave() {
        let (store, root) = store("panic");
        let mut last_good = LastGood::default();
        last_good.keep(copy(&store, true));

        let before = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        install_panic_hook(last_good.crash_copy());
        let result = std::panic::catch_unwind(|| panic!("forced crash"));
        let _ = std::panic::take_hook();
        std::panic::set_hook(before);

        assert!(result.is_err());
        assert_eq!(store.autosave_ids(KEY), vec![0]);
        assert_eq!(store.load_battery(KEY).as_deref(), Some(&b"sram"[..]));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn each_console_keeps_at_most_its_own_count_of_autosaves() {
        for kind in [ConsoleKind::Gb, ConsoleKind::Gbc, ConsoleKind::Gba] {
            assert_eq!(keep_for(kind, 0), 1);
            assert_eq!(keep_for(kind, 7), 7);
            assert_eq!(keep_for(kind, 500), max_keep(kind));
        }
        let (store, root) = store("console-keep");
        let mut copy = copy(&store, true);
        copy.keep = 500;
        for _ in 0..=MAX_KEEP {
            write_copy(&copy).expect("written");
        }
        assert_eq!(store.autosave_ids(KEY).len(), max_keep(ConsoleKind::Gb));
        let _ = std::fs::remove_dir_all(root);
    }
}

use crate::storage::SaveStore;
use std::fmt::Display;
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::time::Duration;
use strum_macros::EnumIter;

pub const DEFAULT_KEEP: usize = 10;
pub const MAX_KEEP: usize = 50;
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
}

pub struct StateCopy {
    pub store: SaveStore,
    pub rom_key: String,
    pub state: Vec<u8>,
    pub frame: Vec<u8>,
    pub battery: Option<Vec<u8>>,
    pub keep: usize,
    pub unsaved: bool,
}

#[derive(Clone, Default)]
pub struct LastGood(Arc<Mutex<Option<StateCopy>>>);

impl LastGood {
    fn hold(&self) -> MutexGuard<'_, Option<StateCopy>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn keep(&self, copy: StateCopy) {
        *self.hold() = Some(copy);
    }

    pub fn clear(&self) {
        *self.hold() = None;
    }

    pub fn mark_saved(&self) {
        if let Some(copy) = self.hold().as_mut() {
            copy.unsaved = false;
        }
    }

    pub fn write_out(&self) -> Result<Option<usize>, String> {
        let mut guard = match self.0.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return Err("the last state is in use".into()),
        };
        let Some(copy) = guard.as_mut().filter(|copy| copy.unsaved) else {
            return Ok(None);
        };
        let battery = match &copy.battery {
            Some(data) => copy.store.store_battery(&copy.rom_key, data),
            None => Ok(()),
        };
        let state = crate::storage::compress(&copy.state);
        let id = copy
            .store
            .store_autosave(&copy.rom_key, &state, &copy.frame, copy.keep)
            .map_err(|err| err.to_string())?;
        battery.map_err(|err| err.to_string())?;
        copy.unsaved = false;
        Ok(Some(id))
    }
}

pub fn install_panic_hook(last_good: LastGood) {
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
pub fn save_when_hidden(last_good: LastGood) {
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
        let root = std::env::temp_dir().join(format!(
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
            state: b"state".to_vec(),
            frame: vec![0; 160 * 144 * 4],
            battery: Some(b"sram".to_vec()),
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
        let last_good = LastGood::default();
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
        let last_good = LastGood::default();
        last_good.keep(copy(&store, true));
        last_good.mark_saved();
        assert_eq!(last_good.write_out(), Ok(None));
        assert!(store.autosave_ids(KEY).is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_held_lock_is_skipped_not_waited_on() {
        let (store, root) = store("locked");
        let last_good = LastGood::default();
        last_good.keep(copy(&store, true));
        let held = last_good.0.lock().expect("lock");
        assert!(last_good.write_out().is_err());
        drop(held);
        assert_eq!(last_good.write_out(), Ok(Some(0)));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_panic_writes_an_autosave() {
        let (store, root) = store("panic");
        let last_good = LastGood::default();
        last_good.keep(copy(&store, true));

        let before = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        install_panic_hook(last_good.clone());
        let result = std::panic::catch_unwind(|| panic!("forced crash"));
        let _ = std::panic::take_hook();
        std::panic::set_hook(before);

        assert!(result.is_err());
        assert_eq!(store.autosave_ids(KEY), vec![0]);
        assert_eq!(store.load_battery(KEY).as_deref(), Some(&b"sram"[..]));
        let _ = std::fs::remove_dir_all(root);
    }
}

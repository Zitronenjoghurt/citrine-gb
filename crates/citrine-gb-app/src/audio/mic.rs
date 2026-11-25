#[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
mod native;
#[cfg(all(feature = "nds", target_arch = "wasm32"))]
mod web;

#[cfg(feature = "nds")]
use citrine_gb::nds::mic::MicPacer;
#[cfg(feature = "nds")]
use citrine_gb_threads::Claim;
#[cfg(feature = "nds")]
use std::sync::Arc;
#[cfg(feature = "nds")]
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

#[cfg(feature = "nds")]
pub const MICROPHONE_HINT: &str = "Which microphone games hear. Off, they hear silence.";
#[cfg(feature = "nds")]
pub const PUSH_TO_TALK_HINT: &str = "On, games hear you only while you hold the Push to talk shortcut, set in Controls, Shortcuts. Off, they hear you whenever they listen.";
#[cfg(feature = "nds")]
pub const PHONE_PUSH_TO_TALK_HINT: &str = "On, games hear you only while you hold the talk button or the Push to talk shortcut. Off, they hear you whenever they listen.";
#[cfg(feature = "nds")]
pub const TALK_BUTTON_HINT: &str = "A button on the game screen to hold while you talk. Without it, hold the Push to talk shortcut on a keyboard or controller, set in Controls, Shortcuts.";
#[cfg(feature = "nds")]
const OFF: &str = "Off";

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Microphone {
    Default,
    Device(String),
    #[default]
    Off,
}

#[cfg(feature = "nds")]
impl Microphone {
    pub fn label(&self) -> &str {
        match self {
            Self::Default => super::SAME_AS_SYSTEM,
            Self::Device(name) => name,
            Self::Off => OFF,
        }
    }

    pub fn choices(devices: &[String]) -> Vec<Self> {
        [Self::Off, Self::Default]
            .into_iter()
            .chain(devices.iter().cloned().map(Self::Device))
            .collect()
    }
}

#[cfg(feature = "nds")]
const DEFAULT_RATE: u32 = 48_000;
#[cfg(feature = "nds")]
const LINGER_SECS: f64 = 2.0;
#[cfg(feature = "nds")]
const CARRY_LIMIT: usize = 48_000;
#[cfg(feature = "nds")]
pub const FLOOR_DB: f32 = -60.0;
#[cfg(feature = "nds")]
const FALL_DB_PER_SEC: f32 = 30.0;
#[cfg(feature = "nds")]
const LONGEST_STEP_SECS: f32 = 0.25;

#[cfg(feature = "nds")]
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f32 = samples.iter().map(|sample| sample * sample).sum();
    (sum / samples.len() as f32).sqrt()
}

#[cfg(feature = "nds")]
pub fn decibels(rms: f32) -> f32 {
    if rms > 0.0 {
        (20.0 * rms.log10()).clamp(FLOOR_DB, 0.0)
    } else {
        FLOOR_DB
    }
}

#[cfg(feature = "nds")]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Meter {
    db: f32,
}

#[cfg(feature = "nds")]
impl Default for Meter {
    fn default() -> Self {
        Self { db: FLOOR_DB }
    }
}

#[cfg(feature = "nds")]
impl Meter {
    pub fn step(&mut self, rms: f32, secs: f32) {
        let heard = decibels(rms);
        let fallen = self.db - FALL_DB_PER_SEC * secs.clamp(0.0, LONGEST_STEP_SECS);
        self.db = heard.max(fallen);
    }

    pub fn fraction(&self) -> f32 {
        ((self.db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0)
    }
}

#[cfg(feature = "nds")]
#[derive(Clone)]
pub struct MicLine {
    pacer: Arc<Claim<MicPacer>>,
    live: Arc<AtomicBool>,
    level: Arc<AtomicU32>,
}

#[cfg(feature = "nds")]
impl Default for MicLine {
    fn default() -> Self {
        Self {
            pacer: Arc::new(Claim::new(MicPacer::new(DEFAULT_RATE))),
            live: Arc::new(AtomicBool::new(false)),
            level: Arc::new(AtomicU32::new(0)),
        }
    }
}

#[cfg(feature = "nds")]
impl MicLine {
    fn with<R>(&self, f: impl FnOnce(&mut MicPacer) -> R) -> R {
        let mut f = Some(f);
        loop {
            if let Some(Some(out)) = self.pacer.try_with(|pacer| f.take().map(|f| f(pacer))) {
                return out;
            }
            std::thread::yield_now();
        }
    }

    fn set_live(&self, live: bool) {
        self.live.store(live, Ordering::Relaxed);
    }

    pub fn restart(&self, rate: u32) {
        self.with(|pacer| *pacer = MicPacer::new(rate));
        self.level.store(0, Ordering::Relaxed);
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    #[cfg(test)]
    pub fn push(&self, samples: &[f32]) {
        self.with(|pacer| pacer.push(samples));
    }

    pub fn push_or_carry(&self, carry: &mut Vec<f32>, samples: &[f32]) {
        if !samples.is_empty() {
            self.level.store(rms(samples).to_bits(), Ordering::Relaxed);
        }
        carry.extend_from_slice(samples);
        if self.pacer.try_with(|pacer| pacer.push(carry)).is_some() {
            carry.clear();
        } else {
            let excess = carry.len().saturating_sub(CARRY_LIMIT);
            carry.drain(..excess);
        }
    }

    pub fn frame(&self, frame_secs: f64, out: &mut Vec<i16>) {
        out.clear();
        let live = self.live.load(Ordering::Relaxed);
        self.with(|pacer| {
            let heard = pacer.frame(frame_secs);
            if live {
                out.extend_from_slice(heard);
            }
        });
    }
}

#[cfg(feature = "nds")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureState {
    #[cfg_attr(
        not(any(target_os = "macos", target_os = "android", target_arch = "wasm32")),
        allow(dead_code)
    )]
    Starting,
    Live,
    #[cfg_attr(
        not(any(target_os = "macos", target_os = "android", target_arch = "wasm32")),
        allow(dead_code)
    )]
    Ended(OpenError),
    Gone,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    Restart,
}

#[cfg(feature = "nds")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenError {
    #[cfg_attr(
        not(any(target_os = "macos", target_os = "android", target_arch = "wasm32")),
        allow(dead_code)
    )]
    Refused,
    NoDevice,
    Failed,
}

#[cfg(feature = "nds")]
pub trait Capture {
    fn state(&mut self) -> CaptureState;
}

#[cfg(feature = "nds")]
pub trait Backend {
    fn open(&mut self, choice: &Microphone, line: &MicLine) -> Result<Box<dyn Capture>, OpenError>;
}

#[cfg(feature = "nds")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    Refused,
    NoDevice,
    Failed,
    FellBack,
}

#[cfg(feature = "nds")]
impl Notice {
    fn of(err: OpenError) -> Self {
        match err {
            OpenError::Refused => Self::Refused,
            OpenError::NoDevice => Self::NoDevice,
            OpenError::Failed => Self::Failed,
        }
    }

    pub fn text(self) -> &'static str {
        match self {
            Self::Refused => REFUSED,
            Self::NoDevice => "No microphone found, so the game hears silence.",
            Self::Failed => FAILED,
            Self::FellBack => "The microphone is gone. Using the system's microphone.",
        }
    }

    pub fn needs_action(self) -> bool {
        self != Self::FellBack
    }
}

#[cfg(feature = "nds")]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MicTest {
    Waiting,
    Level(f32),
    Problem(OpenError),
}

#[cfg(feature = "nds")]
impl MicTest {
    pub fn words(self) -> Option<&'static str> {
        match self {
            Self::Waiting => Some("Waiting for permission"),
            Self::Level(_) => None,
            Self::Problem(OpenError::Refused) => Some(SHORT_REFUSED),
            Self::Problem(OpenError::NoDevice) => Some("No microphone found"),
            Self::Problem(OpenError::Failed) => Some("The microphone didn't open"),
        }
    }

    pub fn fraction(self) -> f32 {
        match self {
            Self::Level(fraction) => fraction,
            _ => 0.0,
        }
    }
}

#[cfg(all(feature = "nds", target_os = "macos"))]
const SHORT_REFUSED: &str = "No permission. Allow it in System Settings, Privacy and Security.";
#[cfg(all(feature = "nds", target_os = "android"))]
const SHORT_REFUSED: &str = "No permission. Allow it in the phone's app settings.";
#[cfg(all(feature = "nds", target_arch = "wasm32"))]
const SHORT_REFUSED: &str = "The browser blocked it. Allow it in the site settings.";
#[cfg(all(
    feature = "nds",
    not(any(target_os = "macos", target_os = "android", target_arch = "wasm32"))
))]
const SHORT_REFUSED: &str = "No permission";

#[cfg(all(feature = "nds", target_os = "macos"))]
const REFUSED: &str = "Citrine may not use the microphone, so the game hears silence. Allow it in System Settings, Privacy and Security, Microphone.";
#[cfg(all(feature = "nds", target_os = "android"))]
const REFUSED: &str = "Citrine may not use the microphone, so the game hears silence. Allow it in the phone's app settings.";
#[cfg(all(feature = "nds", target_arch = "wasm32"))]
const REFUSED: &str =
    "The browser blocked the microphone, so the game hears silence. Allow it in the site settings.";
#[cfg(all(
    feature = "nds",
    not(any(target_os = "macos", target_os = "android", target_arch = "wasm32"))
))]
const REFUSED: &str = "Citrine may not use the microphone, so the game hears silence.";

#[cfg(all(feature = "nds", windows))]
const FAILED: &str = "The microphone didn't open, so the game hears silence. Check Windows Settings, Privacy and security, Microphone.";
#[cfg(all(feature = "nds", not(windows)))]
const FAILED: &str = "The microphone didn't open, so the game hears silence.";

#[cfg(feature = "nds")]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Wanted {
    pub game: Option<u64>,
    pub running: bool,
    pub in_use: bool,
    pub push_to_talk: bool,
    pub talking: bool,
    pub testing: bool,
}

#[cfg(feature = "nds")]
#[derive(Default)]
pub struct Mic {
    line: MicLine,
    capture: Option<Box<dyn Capture>>,
    opened: Option<Microphone>,
    failed: Option<Microphone>,
    game: Option<u64>,
    told: bool,
    last_use: Option<web_time::Instant>,
    last_talk: Option<web_time::Instant>,
    seen: Option<Microphone>,
    asking: bool,
    live: bool,
    testing: bool,
    starting: bool,
    failed_with: Option<OpenError>,
    meter: Meter,
    metered: Option<web_time::Instant>,
    test: Option<MicTest>,
}

#[cfg(feature = "nds")]
impl Mic {
    pub fn line(&self) -> &MicLine {
        &self.line
    }

    pub fn is_open(&self) -> bool {
        self.capture.is_some()
    }

    pub fn is_live(&self) -> bool {
        self.live
    }

    pub fn test(&self) -> Option<MicTest> {
        self.test
    }

    pub fn follow_platform(
        &mut self,
        choice: &Microphone,
        wanted: Wanted,
        now: web_time::Instant,
    ) -> Option<Notice> {
        #[cfg(not(target_arch = "wasm32"))]
        let mut backend = native::Native;
        #[cfg(target_arch = "wasm32")]
        let mut backend = web::Web;
        self.follow(&mut backend, choice, wanted, now)
    }

    pub fn follow(
        &mut self,
        backend: &mut dyn Backend,
        choice: &Microphone,
        wanted: Wanted,
        now: web_time::Instant,
    ) -> Option<Notice> {
        let notice = self.follow_capture(backend, choice, wanted, now);
        self.live = self.capture.is_some()
            && !self.asking
            && self.playing(choice, wanted, now)
            && (!wanted.push_to_talk || wanted.talking);
        self.line.set_live(self.live);
        self.test = self.read_test(choice, wanted.testing, now);
        notice
    }

    fn read_test(
        &mut self,
        choice: &Microphone,
        testing: bool,
        now: web_time::Instant,
    ) -> Option<MicTest> {
        let secs = self
            .metered
            .map_or(0.0, |at| now.duration_since(at).as_secs_f32());
        self.metered = Some(now);
        if !testing || *choice == Microphone::Off {
            self.meter = Meter::default();
            self.metered = None;
            return None;
        }
        if self.failed.as_ref() == Some(choice) {
            return Some(MicTest::Problem(
                self.failed_with.unwrap_or(OpenError::Failed),
            ));
        }
        if self.capture.is_none() || self.starting {
            return Some(MicTest::Waiting);
        }
        self.meter.step(self.line.level(), secs);
        Some(MicTest::Level(self.meter.fraction()))
    }

    fn playing(&self, choice: &Microphone, wanted: Wanted, now: web_time::Instant) -> bool {
        let recent = |at: Option<web_time::Instant>| {
            at.is_some_and(|at| now.duration_since(at).as_secs_f64() < LINGER_SECS)
        };
        *choice != Microphone::Off
            && wanted.game.is_some()
            && wanted.running
            && recent(self.last_use)
            && (!wanted.push_to_talk || recent(self.last_talk))
    }

    fn follow_capture(
        &mut self,
        backend: &mut dyn Backend,
        choice: &Microphone,
        wanted: Wanted,
        now: web_time::Instant,
    ) -> Option<Notice> {
        if self.seen.as_ref() != Some(choice) {
            let was_off = self.seen == Some(Microphone::Off);
            self.seen = Some(choice.clone());
            self.asking = was_off && *choice != Microphone::Off;
            if self.asking {
                self.failed = None;
                self.told = false;
            }
        }
        if wanted.testing && !self.testing {
            self.failed = None;
        }
        self.testing = wanted.testing;
        if wanted.game != self.game {
            self.game = wanted.game;
            self.failed = None;
            self.told = false;
            self.last_use = None;
        }
        if wanted.in_use && wanted.running {
            self.last_use = Some(now);
        }
        if wanted.talking {
            self.last_talk = Some(now);
        }
        let active = *choice != Microphone::Off
            && (self.asking || wanted.testing || self.playing(choice, wanted, now));
        if !active {
            self.close();
            self.starting = false;
            return None;
        }
        if self.opened.as_ref() != Some(choice) && self.opened.is_some() {
            self.close();
            self.failed = None;
        }
        if self.failed.as_ref() == Some(choice) {
            self.asking = false;
            return None;
        }
        let mut notice = None;
        if self.capture.is_none() {
            notice = self.open(backend, choice, choice);
        }
        let state = self.capture.as_mut().map(|capture| capture.state());
        match state {
            Some(CaptureState::Ended(err)) => {
                self.close();
                self.failed = Some(choice.clone());
                self.failed_with = Some(err);
                notice = Some(Notice::of(err));
            }
            Some(CaptureState::Gone) => {
                self.close();
                notice = match self.open(backend, choice, &Microphone::Default) {
                    None => Some(Notice::FellBack),
                    failed => failed,
                };
            }
            Some(CaptureState::Restart) => {
                self.close();
                notice = self.open(backend, choice, choice);
            }
            _ => {}
        }
        self.starting = state == Some(CaptureState::Starting);
        if !self.starting {
            self.asking = false;
        }
        self.tell(notice)
    }

    fn open(
        &mut self,
        backend: &mut dyn Backend,
        choice: &Microphone,
        device: &Microphone,
    ) -> Option<Notice> {
        match backend.open(device, &self.line) {
            Ok(capture) => {
                self.capture = Some(capture);
                self.opened = Some(choice.clone());
                None
            }
            Err(err) => {
                self.failed = Some(choice.clone());
                self.failed_with = Some(err);
                Some(Notice::of(err))
            }
        }
    }

    fn close(&mut self) {
        if self.capture.take().is_some() {
            self.line.restart(DEFAULT_RATE);
        }
        self.opened = None;
    }

    fn tell(&mut self, notice: Option<Notice>) -> Option<Notice> {
        let notice = notice?;
        if notice != Notice::FellBack {
            if self.told {
                return None;
            }
            self.told = true;
        }
        Some(notice)
    }
}

#[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
pub fn input_devices() -> Vec<String> {
    native::input_devices()
}

#[cfg(all(feature = "nds", target_arch = "wasm32"))]
pub fn input_devices() -> Vec<String> {
    web::input_devices()
}

#[cfg(all(test, feature = "nds"))]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;

    #[derive(Default)]
    struct Script {
        opened: Vec<Microphone>,
        refuse: bool,
        missing: bool,
        state: Option<CaptureState>,
        open_now: usize,
    }

    struct Fake(Rc<RefCell<Script>>);

    struct FakeCapture(Rc<RefCell<Script>>);

    impl Capture for FakeCapture {
        fn state(&mut self) -> CaptureState {
            self.0
                .borrow_mut()
                .state
                .take()
                .unwrap_or(CaptureState::Live)
        }
    }

    impl Drop for FakeCapture {
        fn drop(&mut self) {
            self.0.borrow_mut().open_now -= 1;
        }
    }

    impl Backend for Fake {
        fn open(
            &mut self,
            choice: &Microphone,
            _line: &MicLine,
        ) -> Result<Box<dyn Capture>, OpenError> {
            let mut script = self.0.borrow_mut();
            script.opened.push(choice.clone());
            if script.refuse {
                return Err(OpenError::Refused);
            }
            if script.missing {
                return Err(OpenError::NoDevice);
            }
            script.open_now += 1;
            Ok(Box::new(FakeCapture(self.0.clone())))
        }
    }

    struct Rig {
        mic: Mic,
        script: Rc<RefCell<Script>>,
        backend: Fake,
        start: web_time::Instant,
    }

    impl Rig {
        fn new() -> Self {
            let script = Rc::new(RefCell::new(Script::default()));
            Self {
                mic: Mic::default(),
                backend: Fake(script.clone()),
                script,
                start: web_time::Instant::now(),
            }
        }

        fn step(&mut self, choice: &Microphone, wanted: Wanted, secs: f64) -> Option<Notice> {
            let now = self.start + Duration::from_secs_f64(secs);
            self.mic.follow(&mut self.backend, choice, wanted, now)
        }

        fn opened(&self) -> Vec<Microphone> {
            self.script.borrow().opened.clone()
        }

        fn open_now(&self) -> usize {
            self.script.borrow().open_now
        }
    }

    const LISTENING: Wanted = Wanted {
        game: Some(1),
        running: true,
        in_use: true,
        push_to_talk: false,
        talking: false,
        testing: false,
    };

    const QUIET: Wanted = Wanted {
        in_use: false,
        ..LISTENING
    };

    const HOLDING: Wanted = Wanted {
        push_to_talk: true,
        talking: true,
        ..LISTENING
    };

    const LET_GO: Wanted = Wanted {
        talking: false,
        ..HOLDING
    };

    fn heard(mic: &Mic) -> Vec<i16> {
        mic.line().push(&[0.5; 4_800]);
        let mut heard = Vec::new();
        mic.line().frame(1.0 / 60.0, &mut heard);
        heard
    }

    #[test]
    fn nothing_opens_until_a_ds_game_reads_the_microphone() {
        let mut rig = Rig::new();
        for frame in 0..120 {
            rig.step(&Microphone::Default, QUIET, f64::from(frame) / 60.0);
        }
        rig.step(&Microphone::Default, Wanted::default(), 3.0);
        assert!(rig.opened().is_empty());
        rig.step(&Microphone::Default, LISTENING, 4.0);
        assert_eq!(rig.opened(), vec![Microphone::Default]);
        assert_eq!(rig.open_now(), 1);
    }

    #[test]
    fn off_never_opens_and_closes_an_open_one() {
        let mut rig = Rig::new();
        rig.step(&Microphone::Off, LISTENING, 0.0);
        assert!(rig.opened().is_empty());
        rig.step(&Microphone::Default, LISTENING, 1.0);
        assert_eq!(rig.open_now(), 1);
        assert_eq!(rig.step(&Microphone::Off, LISTENING, 1.1), None);
        assert_eq!(rig.open_now(), 0);
        assert!(!rig.mic.is_open());
    }

    #[test]
    fn capture_stops_when_the_game_stops_or_goes_quiet() {
        let mut rig = Rig::new();
        rig.step(&Microphone::Default, LISTENING, 0.0);
        rig.step(
            &Microphone::Default,
            Wanted {
                running: false,
                ..LISTENING
            },
            0.5,
        );
        assert_eq!(rig.open_now(), 0);
        rig.step(&Microphone::Default, LISTENING, 1.0);
        rig.step(&Microphone::Default, QUIET, 2.5);
        assert_eq!(rig.open_now(), 1);
        rig.step(&Microphone::Default, QUIET, 3.5);
        assert_eq!(rig.open_now(), 0);
        rig.step(&Microphone::Default, LISTENING, 4.0);
        rig.step(&Microphone::Default, Wanted::default(), 4.1);
        assert_eq!(rig.open_now(), 0);
    }

    #[test]
    fn a_refused_microphone_says_so_once_and_stops_asking() {
        let mut rig = Rig::new();
        rig.script.borrow_mut().refuse = true;
        assert_eq!(
            rig.step(&Microphone::Default, LISTENING, 0.0),
            Some(Notice::Refused)
        );
        for frame in 1..600 {
            assert_eq!(
                rig.step(&Microphone::Default, LISTENING, f64::from(frame) / 60.0),
                None
            );
        }
        assert_eq!(rig.opened().len(), 1);
        assert!(Notice::Refused.needs_action());
    }

    #[test]
    fn a_refusal_after_the_prompt_says_so_once() {
        let mut rig = Rig::new();
        rig.script.borrow_mut().state = Some(CaptureState::Starting);
        assert_eq!(rig.step(&Microphone::Default, LISTENING, 0.0), None);
        rig.script.borrow_mut().state = Some(CaptureState::Ended(OpenError::Refused));
        assert_eq!(
            rig.step(&Microphone::Default, LISTENING, 0.1),
            Some(Notice::Refused)
        );
        assert_eq!(rig.open_now(), 0);
        assert_eq!(rig.step(&Microphone::Default, LISTENING, 0.2), None);
        assert_eq!(rig.opened().len(), 1);
    }

    #[test]
    fn a_new_game_or_a_new_choice_asks_again() {
        let mut rig = Rig::new();
        rig.script.borrow_mut().refuse = true;
        rig.step(&Microphone::Default, LISTENING, 0.0);
        rig.script.borrow_mut().refuse = false;
        let other = Microphone::Device("USB mic".into());
        assert_eq!(rig.step(&other, LISTENING, 0.1), None);
        assert_eq!(rig.open_now(), 1);
        rig.script.borrow_mut().refuse = true;
        rig.step(
            &Microphone::Default,
            Wanted {
                game: Some(2),
                ..LISTENING
            },
            0.2,
        );
        assert_eq!(
            rig.step(
                &Microphone::Default,
                Wanted {
                    game: Some(3),
                    ..LISTENING
                },
                0.3
            ),
            Some(Notice::Refused)
        );
    }

    #[test]
    fn no_device_says_so_once() {
        let mut rig = Rig::new();
        rig.script.borrow_mut().missing = true;
        assert_eq!(
            rig.step(&Microphone::Default, LISTENING, 0.0),
            Some(Notice::NoDevice)
        );
        assert_eq!(rig.step(&Microphone::Default, LISTENING, 0.1), None);
    }

    #[test]
    fn an_unplugged_device_falls_back_to_the_default_and_keeps_the_choice() {
        let mut rig = Rig::new();
        let usb = Microphone::Device("USB mic".into());
        rig.step(&usb, LISTENING, 0.0);
        rig.script.borrow_mut().state = Some(CaptureState::Gone);
        assert_eq!(rig.step(&usb, LISTENING, 0.1), Some(Notice::FellBack));
        assert_eq!(rig.opened(), vec![usb.clone(), Microphone::Default]);
        assert_eq!(rig.open_now(), 1);
        assert_eq!(rig.step(&usb, LISTENING, 0.2), None);
        assert_eq!(rig.opened().len(), 2);
        assert!(!Notice::FellBack.needs_action());
    }

    #[test]
    fn closing_clears_what_the_game_would_hear() {
        let mut rig = Rig::new();
        rig.step(&Microphone::Default, LISTENING, 0.0);
        rig.mic.line().push(&[0.5; 4_800]);
        rig.step(&Microphone::Off, LISTENING, 0.1);
        let mut heard = Vec::new();
        rig.mic.line().frame(1.0 / 60.0, &mut heard);
        assert!(heard.iter().all(|&sample| sample == 0));
    }

    #[test]
    fn a_busy_line_carries_samples_to_the_next_push() {
        let line = MicLine::default();
        let mut carry = Vec::new();
        let other = line.clone();
        line.pacer
            .try_with(|_| other.push_or_carry(&mut carry, &[0.5; 100]));
        assert_eq!(carry.len(), 100);
        line.push_or_carry(&mut carry, &[0.5; 900]);
        assert!(carry.is_empty());
        assert_eq!(line.with(|pacer| pacer.kept()), 1_000);
    }

    #[test]
    fn choices_list_off_then_default_then_devices() {
        let devices = vec!["A".to_owned(), "B".to_owned()];
        let labels: Vec<String> = Microphone::choices(&devices)
            .iter()
            .map(|choice| choice.label().to_owned())
            .collect();
        assert_eq!(labels, ["Off", "Same as system", "A", "B"]);
        assert_eq!(Microphone::default(), Microphone::Off);
    }

    #[test]
    fn turning_it_on_asks_at_once_and_closes_after_the_answer() {
        let mut rig = Rig::new();
        rig.step(&Microphone::Off, Wanted::default(), 0.0);
        assert!(rig.opened().is_empty());
        rig.script.borrow_mut().state = Some(CaptureState::Starting);
        assert_eq!(rig.step(&Microphone::Default, Wanted::default(), 0.1), None);
        assert_eq!(rig.open_now(), 1);
        assert!(!rig.mic.is_live());
        assert_eq!(rig.step(&Microphone::Default, Wanted::default(), 5.0), None);
        rig.step(&Microphone::Default, Wanted::default(), 5.1);
        assert_eq!(rig.open_now(), 0);
        assert_eq!(rig.opened().len(), 1);
    }

    #[test]
    fn turning_it_on_says_at_once_when_it_is_refused() {
        let mut rig = Rig::new();
        rig.step(&Microphone::Off, QUIET, 0.0);
        rig.script.borrow_mut().refuse = true;
        assert_eq!(
            rig.step(&Microphone::Default, QUIET, 0.1),
            Some(Notice::Refused)
        );
        assert_eq!(rig.step(&Microphone::Default, LISTENING, 0.2), None);
        assert_eq!(rig.opened().len(), 1);
    }

    #[test]
    fn a_microphone_already_on_at_start_asks_nothing_until_a_game_listens() {
        let mut rig = Rig::new();
        rig.step(&Microphone::Default, Wanted::default(), 0.0);
        rig.step(&Microphone::Default, QUIET, 0.1);
        assert!(rig.opened().is_empty());
    }

    #[test]
    fn the_game_hears_only_while_the_microphone_is_live() {
        let mut rig = Rig::new();
        rig.step(&Microphone::Default, LISTENING, 0.0);
        assert!(rig.mic.is_live());
        assert!(heard(&rig.mic).iter().any(|&sample| sample != 0));
        rig.step(&Microphone::Off, LISTENING, 0.1);
        assert!(!rig.mic.is_live());
        assert!(heard(&rig.mic).is_empty());
    }

    #[test]
    fn push_to_talk_opens_on_the_press_and_hears_only_while_held() {
        let mut rig = Rig::new();
        for frame in 0..60 {
            rig.step(&Microphone::Default, LET_GO, f64::from(frame) / 60.0);
        }
        assert!(rig.opened().is_empty());
        rig.step(&Microphone::Default, HOLDING, 1.0);
        assert!(rig.mic.is_live());
        assert!(heard(&rig.mic).iter().any(|&sample| sample != 0));
        rig.step(&Microphone::Default, LET_GO, 1.1);
        assert!(!rig.mic.is_live());
        assert!(heard(&rig.mic).is_empty());
        assert_eq!(rig.open_now(), 1);
        rig.step(&Microphone::Default, LET_GO, 3.5);
        assert_eq!(rig.open_now(), 0);
        rig.step(&Microphone::Default, HOLDING, 4.0);
        assert!(rig.mic.is_live());
        assert_eq!(rig.opened().len(), 2);
    }

    #[test]
    fn push_to_talk_opens_nothing_while_the_game_does_not_listen() {
        let mut rig = Rig::new();
        rig.step(
            &Microphone::Default,
            Wanted {
                in_use: false,
                ..HOLDING
            },
            0.0,
        );
        assert!(rig.opened().is_empty());
        assert!(!rig.mic.is_live());
    }

    const TESTING: Wanted = Wanted {
        game: None,
        running: false,
        in_use: false,
        push_to_talk: false,
        talking: false,
        testing: true,
    };

    fn speak(mic: &Mic, level: f32) {
        mic.line().push_or_carry(&mut Vec::new(), &[level; 480]);
    }

    #[test]
    fn the_level_is_the_loudness_in_decibels_over_sixty() {
        assert_eq!(rms(&[]), 0.0);
        assert!((rms(&[0.5, -0.5, 0.5, -0.5]) - 0.5).abs() < 1e-6);
        assert_eq!(decibels(1.0), 0.0);
        assert!((decibels(0.1) + 20.0).abs() < 1e-4);
        assert_eq!(decibels(0.0), FLOOR_DB);
        assert_eq!(decibels(1e-6), FLOOR_DB);
        assert_eq!(decibels(2.0), 0.0);

        let mut meter = Meter::default();
        assert_eq!(meter.fraction(), 0.0);
        meter.step(0.1, 0.016);
        assert!((meter.fraction() - 2.0 / 3.0).abs() < 1e-4, "rises at once");
        meter.step(1.0, 0.016);
        assert_eq!(meter.fraction(), 1.0);
        meter.step(0.0, 0.1);
        assert!(
            (meter.fraction() - 0.95).abs() < 1e-4,
            "falls 30 dB a second"
        );
        meter.step(0.0, 10.0);
        assert!(
            (meter.fraction() - 0.825).abs() < 1e-4,
            "a stalled frame falls a quarter second"
        );
        for _ in 0..20 {
            meter.step(0.0, 0.25);
        }
        assert_eq!(meter.fraction(), 0.0);
    }

    #[test]
    fn the_settings_page_hears_the_microphone_and_closing_it_stops_capture() {
        let mut rig = Rig::new();
        rig.step(&Microphone::Default, Wanted::default(), 0.0);
        assert_eq!(rig.mic.test(), None);
        rig.step(&Microphone::Default, TESTING, 0.1);
        assert_eq!(rig.open_now(), 1);
        assert!(!rig.mic.is_live(), "no game hears the test");
        assert_eq!(rig.mic.test(), Some(MicTest::Level(0.0)));
        speak(&rig.mic, 0.1);
        rig.step(&Microphone::Default, TESTING, 0.2);
        let Some(MicTest::Level(heard)) = rig.mic.test() else {
            panic!("{:?}", rig.mic.test());
        };
        assert!((heard - 2.0 / 3.0).abs() < 1e-4, "{heard}");
        for frame in 0..600 {
            rig.step(&Microphone::Default, TESTING, 0.3 + f64::from(frame) / 60.0);
        }
        assert_eq!(rig.open_now(), 1, "the page keeps it open");
        rig.step(&Microphone::Default, Wanted::default(), 11.0);
        assert_eq!(rig.open_now(), 0);
        assert_eq!(rig.mic.test(), None);
        assert_eq!(rig.mic.line().level(), 0.0);
        rig.step(&Microphone::Off, TESTING, 12.0);
        assert_eq!(rig.open_now(), 0);
        assert_eq!(rig.mic.test(), None);
    }

    #[test]
    fn the_test_says_why_it_hears_nothing() {
        let mut rig = Rig::new();
        rig.script.borrow_mut().state = Some(CaptureState::Starting);
        rig.step(&Microphone::Default, TESTING, 0.0);
        assert_eq!(rig.mic.test(), Some(MicTest::Waiting));
        rig.script.borrow_mut().state = Some(CaptureState::Ended(OpenError::Refused));
        rig.step(&Microphone::Default, TESTING, 0.1);
        assert_eq!(rig.mic.test(), Some(MicTest::Problem(OpenError::Refused)));
        rig.step(&Microphone::Default, TESTING, 0.2);
        assert_eq!(
            rig.opened().len(),
            1,
            "no asking again while the page stays open"
        );
        assert!(MicTest::Problem(OpenError::Refused).words().is_some());

        rig.step(&Microphone::Default, Wanted::default(), 0.3);
        rig.script.borrow_mut().missing = true;
        rig.step(&Microphone::Default, TESTING, 0.4);
        assert_eq!(rig.opened().len(), 2, "opening the page again tries again");
        assert_eq!(rig.mic.test(), Some(MicTest::Problem(OpenError::NoDevice)));
        assert_eq!(
            MicTest::Problem(OpenError::NoDevice).words(),
            Some("No microphone found")
        );
        assert_eq!(MicTest::Level(0.5).words(), None);
    }
}

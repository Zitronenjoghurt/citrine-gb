use crate::app::ui_state::keybinds::{
    BindingKind, BindingTarget, GbButton, KeyBindings, PendingBinding,
};
use crate::pad_input::{PadAction, PadAxis, PadInput};
use std::collections::VecDeque;
use std::fmt::{Display, Formatter, Write};
use std::time::Duration;
use strum::IntoEnumIterator;
use web_time::Instant;

pub const LOG_CAP: usize = 1000;
pub const REPORT_FILE: &str = "citrine-controller-report.txt";

#[derive(Debug, Clone, PartialEq)]
pub enum EntryKind {
    Pad {
        gamepad: usize,
        input: PadInput,
        code: Option<String>,
        action: PadAction,
        changed: bool,
    },
    Still {
        gamepad: usize,
        axis: PadAxis,
        code: Option<String>,
        action: PadAction,
        moves: u32,
        low: f32,
        high: f32,
    },
    Focus(bool),
}

#[derive(Debug, Clone, PartialEq)]
pub struct LogEntry {
    pub at: Duration,
    pub kind: EntryKind,
}

impl LogEntry {
    pub fn time(&self) -> String {
        format!("{:>8.3}s", self.at.as_secs_f64())
    }

    pub fn event(&self) -> String {
        match &self.kind {
            EntryKind::Pad {
                gamepad,
                input,
                code,
                action,
                changed,
            } => format!(
                "pad {gamepad}  {input}{}  =>  {}",
                code_text(code),
                action.describe(*changed)
            ),
            EntryKind::Still {
                gamepad,
                axis,
                code,
                action,
                moves,
                low,
                high,
            } => {
                let moved = if *moves == 1 {
                    format!("{axis:?} at {low:+.2}")
                } else {
                    format!("{axis:?} moved {moves} times between {low:+.2} and {high:+.2}")
                };
                format!(
                    "pad {gamepad}  {moved}{}  =>  {}",
                    code_text(code),
                    action.describe(false)
                )
            }
            EntryKind::Focus(true) => "window focused".to_string(),
            EntryKind::Focus(false) => "window lost focus".to_string(),
        }
    }
}

fn code_text(code: &Option<String>) -> String {
    code.as_ref()
        .map(|code| format!("  code {code}"))
        .unwrap_or_default()
}

impl Display for LogEntry {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}  {}", self.time(), self.event())
    }
}

pub struct PadLog {
    started: Instant,
    entries: VecDeque<LogEntry>,
    dropped: usize,
    seen: Vec<usize>,
}

impl PadLog {
    pub fn new(started: Instant) -> Self {
        Self {
            started,
            entries: VecDeque::new(),
            dropped: 0,
            seen: Vec::new(),
        }
    }

    pub fn record(
        &mut self,
        now: Instant,
        gamepad: usize,
        input: PadInput,
        code: Option<String>,
        action: PadAction,
        changed: bool,
    ) {
        self.note_seen(gamepad);
        self.push(
            now,
            EntryKind::Pad {
                gamepad,
                input,
                code,
                action,
                changed,
            },
        );
    }

    fn note_seen(&mut self, gamepad: usize) {
        if !self.seen.contains(&gamepad) {
            self.seen.push(gamepad);
        }
    }

    pub fn record_still(
        &mut self,
        now: Instant,
        gamepad: usize,
        axis: PadAxis,
        value: f32,
        code: Option<String>,
        action: PadAction,
    ) {
        self.note_seen(gamepad);
        let still_run = self
            .entries
            .iter_mut()
            .rev()
            .take_while(|entry| matches!(entry.kind, EntryKind::Still { .. }));
        for entry in still_run {
            if let EntryKind::Still {
                gamepad: still_pad,
                axis: still_axis,
                code: still_code,
                action: still_action,
                moves,
                low,
                high,
            } = &mut entry.kind
                && *still_pad == gamepad
                && *still_axis == axis
            {
                *still_code = code;
                *still_action = action;
                *moves += 1;
                *low = low.min(value);
                *high = high.max(value);
                return;
            }
        }
        self.push(
            now,
            EntryKind::Still {
                gamepad,
                axis,
                code,
                action,
                moves: 1,
                low: value,
                high: value,
            },
        );
    }

    pub fn push(&mut self, now: Instant, kind: EntryKind) {
        if self.entries.len() == LOG_CAP {
            self.entries.pop_front();
            self.dropped += 1;
        }
        self.entries.push_back(LogEntry {
            at: now.saturating_duration_since(self.started),
            kind,
        });
    }

    pub fn clear(&mut self, now: Instant) {
        self.entries.clear();
        self.dropped = 0;
        self.started = now;
    }

    pub fn entries(&self) -> impl DoubleEndedIterator<Item = &LogEntry> {
        self.entries.iter()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn dropped(&self) -> usize {
        self.dropped
    }

    pub fn seen(&self) -> &[usize] {
        &self.seen
    }
}

#[derive(Default)]
pub struct ControllerDebugState {
    log: Option<PadLog>,
    focused: Option<bool>,
}

impl ControllerDebugState {
    pub fn set_open(&mut self, open: bool, now: Instant) {
        match (open, self.log.is_some()) {
            (true, false) => self.log = Some(PadLog::new(now)),
            (false, true) => {
                self.log = None;
                self.focused = None;
            }
            _ => {}
        }
    }

    pub fn is_recording(&self) -> bool {
        self.log.is_some()
    }

    pub fn log(&self) -> Option<&PadLog> {
        self.log.as_ref()
    }

    pub fn log_mut(&mut self) -> Option<&mut PadLog> {
        self.log.as_mut()
    }

    pub fn note_focus(&mut self, focused: Option<bool>, now: Instant) {
        let Some(log) = self.log.as_mut() else {
            return;
        };
        if let Some(focused) = focused
            && self.focused != Some(focused)
        {
            if self.focused.is_some() {
                log.push(now, EntryKind::Focus(focused));
            }
            self.focused = Some(focused);
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ButtonState {
    pub name: String,
    pub code: String,
    pub pressed: bool,
    pub value: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AxisState {
    pub axis: PadAxis,
    pub name: String,
    pub code: String,
    pub value: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawState {
    pub code: String,
    pub value: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PadInfo {
    pub id: usize,
    pub name: String,
    pub details: Vec<(&'static str, String)>,
    pub buttons: Vec<ButtonState>,
    pub axes: Vec<AxisState>,
    pub raw: Vec<RawState>,
}

impl PadInfo {
    pub fn pressed(&self) -> Vec<String> {
        self.buttons
            .iter()
            .filter(|b| b.pressed)
            .map(|b| b.name.clone())
            .collect()
    }
}

pub fn usb_id(vendor: Option<u16>, product: Option<u16>) -> String {
    let hex = |id: Option<u16>| id.map_or("unknown".to_string(), |id| format!("{id:04x}"));
    format!("{}:{}", hex(vendor), hex(product))
}

pub fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SystemInfo {
    pub version: &'static str,
    pub build: String,
    pub os: String,
    pub backend: String,
}

impl SystemInfo {
    pub fn current(backend: String) -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION"),
            build: build_target(),
            os: os_version(),
            backend,
        }
    }
}

fn build_target() -> String {
    let kind = if cfg!(target_arch = "wasm32") {
        "web"
    } else {
        "native"
    };
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let env = if cfg!(target_env = "msvc") {
        "msvc"
    } else if cfg!(target_env = "gnu") {
        "gnu"
    } else {
        ""
    };
    let target: Vec<&str> = [std::env::consts::OS, std::env::consts::ARCH, env]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect();
    format!("{}, {kind}, {profile}", target.join(" "))
}

#[cfg(not(target_arch = "wasm32"))]
fn os_version() -> String {
    os_info::get().to_string()
}

#[cfg(target_arch = "wasm32")]
fn os_version() -> String {
    web_sys::window()
        .and_then(|w| w.navigator().user_agent().ok())
        .unwrap_or_else(|| "unknown browser".to_string())
}

pub fn capture_text(pending: Option<PendingBinding>) -> String {
    let Some(pending) = pending else {
        return "none".to_string();
    };
    let target = match pending.target {
        BindingTarget::Button(button) => format!("Game Boy {button}"),
        BindingTarget::Shortcut(action) => action.label().to_string(),
    };
    let kind = match pending.kind {
        BindingKind::Keyboard => "a key",
        BindingKind::Gamepad => "a controller button",
        BindingKind::Any => "a key or controller button",
    };
    format!("waiting for {kind} for {target}")
}

pub fn held_text(held: &[GbButton]) -> String {
    if held.is_empty() {
        return "none".to_string();
    }
    let names: Vec<String> = held.iter().map(|b| b.to_string()).collect();
    names.join(", ")
}

pub fn steer_text(bindings: &KeyBindings) -> String {
    if bindings.stick_as_dpad {
        format!("on, dead zone {}%", bindings.deadzone_percent())
    } else {
        "off".to_string()
    }
}

pub struct Report<'a> {
    pub system: &'a SystemInfo,
    pub focused: Option<bool>,
    pub pads: &'a [PadInfo],
    pub bindings: &'a KeyBindings,
    pub capture: Option<PendingBinding>,
    pub held: &'a [GbButton],
    pub log: Option<&'a PadLog>,
}

impl Display for Report<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut out = String::new();
        let s = self.system;
        writeln!(out, "Citrine controller report")?;
        writeln!(out)?;
        writeln!(out, "== System")?;
        writeln!(out, "Citrine: {}", s.version)?;
        writeln!(out, "Build: {}", s.build)?;
        writeln!(out, "OS: {}", s.os)?;
        writeln!(out, "Controller API: {}", s.backend)?;
        let focus = self.focused.map_or("unknown", yes_no);
        writeln!(out, "Window focused: {focus}")?;
        writeln!(out, "Binding capture: {}", capture_text(self.capture))?;
        writeln!(out, "Game Boy buttons held: {}", held_text(self.held))?;
        writeln!(out)?;

        let deadzone = self.bindings.deadzone();
        writeln!(out, "== Controllers ({})", self.pads.len())?;
        if self.pads.is_empty() {
            writeln!(out, "none found")?;
        }
        for pad in self.pads {
            writeln!(out, "Pad {}: {}", pad.id, pad.name)?;
            for (label, value) in &pad.details {
                writeln!(out, "  {label}: {value}")?;
            }
            let pressed = pad.pressed();
            let pressed = if pressed.is_empty() {
                "none".to_string()
            } else {
                pressed.join(", ")
            };
            writeln!(out, "  Pressed now: {pressed}")?;
            let buttons: Vec<String> = pad
                .buttons
                .iter()
                .map(|b| format!("{} {}", b.name, b.code))
                .collect();
            writeln!(out, "  Buttons ({}): {}", buttons.len(), buttons.join(", "))?;
            let axes: Vec<String> = pad
                .axes
                .iter()
                .map(|a| format!("{} {} at {:+.2}", a.name, a.code, a.value))
                .collect();
            writeln!(out, "  Axes ({}): {}", axes.len(), axes.join(", "))?;
            let stick: Vec<String> = pad
                .axes
                .iter()
                .filter(|a| a.axis.is_left_stick())
                .map(|a| {
                    let side = if a.value.abs() > deadzone {
                        "past"
                    } else {
                        "inside"
                    };
                    format!("{} {:+.2} {side}", a.name, a.value)
                })
                .collect();
            if !stick.is_empty() {
                writeln!(
                    out,
                    "  Left stick against the dead zone ({deadzone:.2}): {}",
                    stick.join(", ")
                )?;
            }
            if !pad.raw.is_empty() {
                let raw: Vec<String> = pad
                    .raw
                    .iter()
                    .map(|r| format!("{} at {:.2}", r.code, r.value))
                    .collect();
                writeln!(out, "  Unnamed codes: {}", raw.join(", "))?;
            }
        }
        writeln!(out)?;

        writeln!(out, "== Bindings")?;
        writeln!(out, "Left stick steers: {}", steer_text(self.bindings))?;
        for button in GbButton::iter() {
            let keys: Vec<&str> = self
                .bindings
                .keys_for(button)
                .iter()
                .map(|k| k.name())
                .collect();
            let pads: Vec<&str> = self
                .bindings
                .pads_for(button)
                .iter()
                .map(|p| p.short_name())
                .collect();
            let list = |items: Vec<&str>| {
                if items.is_empty() {
                    "none".to_string()
                } else {
                    items.join(", ")
                }
            };
            writeln!(
                out,
                "{button}: keys {}; controller {}",
                list(keys),
                list(pads)
            )?;
        }
        writeln!(out)?;

        match self.log {
            None => writeln!(out, "== Event log\nnot recording")?,
            Some(log) => {
                let noun = if log.len() == 1 { "event" } else { "events" };
                writeln!(out, "== Event log, {} {noun}", log.len())?;
                if log.dropped() > 0 {
                    writeln!(out, "{} older events dropped", log.dropped())?;
                }
                if log.is_empty() {
                    writeln!(out, "no events yet")?;
                }
                for entry in log.entries() {
                    writeln!(out, "{entry}")?;
                }
            }
        }
        f.write_str(&out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ui_state::keybinds::PadButton;
    use crate::pad_input::{Ignored, PadKey};

    fn pad_entry(action: PadAction) -> EntryKind {
        EntryKind::Pad {
            gamepad: 0,
            input: PadInput::Pressed(PadKey::Known(PadButton::East)),
            code: Some("Button(1)".to_string()),
            action,
            changed: true,
        }
    }

    fn sample_pad() -> PadInfo {
        PadInfo {
            id: 0,
            name: "Xbox Wireless Controller".to_string(),
            details: vec![
                ("OS name", "Xbox Wireless Controller".to_string()),
                ("USB ID", usb_id(Some(0x045e), Some(0x0b13))),
                ("UUID", "12121212-1212-1212-1212-121212121212".to_string()),
                ("Power", "battery at 80%".to_string()),
            ],
            buttons: vec![ButtonState {
                name: "East".to_string(),
                code: "Button(1)".to_string(),
                pressed: true,
                value: 1.0,
            }],
            axes: vec![AxisState {
                axis: PadAxis::LeftStickX,
                name: "LeftStickX".to_string(),
                code: "Axis(0)".to_string(),
                value: -0.25,
            }],
            raw: Vec::new(),
        }
    }

    #[test]
    fn closing_the_tab_drops_the_log() {
        let start = Instant::now();
        let mut state = ControllerDebugState::default();
        assert!(state.log_mut().is_none(), "nothing records while closed");

        state.set_open(true, start);
        state
            .log_mut()
            .expect("recording")
            .push(start, pad_entry(PadAction::Press(GbButton::A)));
        state.set_open(true, start + Duration::from_secs(1));
        assert_eq!(
            state.log().map(PadLog::len),
            Some(1),
            "staying open keeps it"
        );

        state.set_open(false, start + Duration::from_secs(2));
        assert!(state.log().is_none());

        state.set_open(true, start + Duration::from_secs(3));
        assert_eq!(
            state.log().map(PadLog::len),
            Some(0),
            "reopening starts fresh"
        );
    }

    #[test]
    fn the_log_keeps_only_the_newest_events() {
        let start = Instant::now();
        let mut log = PadLog::new(start);
        for i in 0..LOG_CAP + 5 {
            log.push(
                start + Duration::from_millis(i as u64),
                pad_entry(PadAction::Press(GbButton::A)),
            );
        }
        assert_eq!(log.len(), LOG_CAP);
        assert_eq!(log.dropped(), 5);
        assert_eq!(
            log.entries().next().map(|e| e.at),
            Some(Duration::from_millis(5))
        );
    }

    #[test]
    fn focus_changes_are_logged_only_while_open() {
        let start = Instant::now();
        let mut state = ControllerDebugState::default();
        state.note_focus(Some(true), start);
        state.set_open(true, start);
        state.note_focus(Some(true), start);
        state.note_focus(Some(true), start);
        state.note_focus(Some(false), start + Duration::from_secs(1));
        let log = state.log().expect("recording");
        let kinds: Vec<&EntryKind> = log.entries().map(|e| &e.kind).collect();
        assert_eq!(kinds, vec![&EntryKind::Focus(false)]);
    }

    #[test]
    fn a_log_line_names_the_event_and_what_happened() {
        let entry = LogEntry {
            at: Duration::from_millis(1500),
            kind: pad_entry(PadAction::Press(GbButton::A)),
        };
        assert_eq!(
            entry.to_string(),
            "   1.500s  pad 0  East pressed  code Button(1)  =>  Game Boy A down"
        );

        let ignored = LogEntry {
            at: Duration::ZERO,
            kind: EntryKind::Pad {
                gamepad: 1,
                input: PadInput::Pressed(PadKey::Known(PadButton::LeftTrigger)),
                code: None,
                action: PadAction::Ignore(Ignored::Unbound),
                changed: false,
            },
        };
        assert!(
            ignored
                .to_string()
                .ends_with("pad 1  LeftTrigger pressed  =>  ignored: not bound")
        );
    }

    #[test]
    fn the_report_has_every_section() {
        let start = Instant::now();
        let mut log = PadLog::new(start);
        log.push(start, pad_entry(PadAction::Press(GbButton::A)));
        let system = SystemInfo {
            version: "0.7.1",
            build: "windows x86_64 msvc, native, release".to_string(),
            os: "Windows 11 Pro 10.0.22631 [64-bit]".to_string(),
            backend: "SDL 3.4.18".to_string(),
        };
        let pads = [sample_pad()];
        let bindings = KeyBindings::default();
        let report = Report {
            system: &system,
            focused: Some(true),
            pads: &pads,
            bindings: &bindings,
            capture: Some(PendingBinding {
                target: BindingTarget::Button(GbButton::B),
                kind: BindingKind::Any,
            }),
            held: &[GbButton::A],
            log: Some(&log),
        }
        .to_string();

        for expected in [
            "Citrine: 0.7.1",
            "Build: windows x86_64 msvc, native, release",
            "OS: Windows 11 Pro 10.0.22631 [64-bit]",
            "Controller API: SDL 3.4.18",
            "Window focused: yes",
            "Binding capture: waiting for a key or controller button for Game Boy B",
            "Game Boy buttons held: A",
            "== Controllers (1)",
            "Pad 0: Xbox Wireless Controller",
            "USB ID: 045e:0b13",
            "UUID: 12121212-1212-1212-1212-121212121212",
            "Power: battery at 80%",
            "Pressed now: East",
            "Axes (1): LeftStickX Axis(0) at -0.25",
            "Left stick against the dead zone (0.50): LeftStickX -0.25 inside",
            "== Bindings",
            "Left stick steers: on, dead zone 50%",
            "A: keys Q, Z, Y, O; controller East, West",
            "== Event log, 1 event",
            "=>  Game Boy A down",
        ] {
            assert!(
                report.contains(expected),
                "missing {expected:?} in\n{report}"
            );
        }
    }

    #[test]
    fn an_empty_report_still_reads_well() {
        let system = SystemInfo::current("browser Gamepad API".to_string());
        let report = Report {
            system: &system,
            focused: None,
            pads: &[],
            bindings: &KeyBindings::default(),
            capture: None,
            held: &[],
            log: None,
        }
        .to_string();
        assert!(report.contains("== Controllers (0)\nnone found"));
        assert!(report.contains("Binding capture: none"));
        assert!(report.contains("Window focused: unknown"));
        assert!(report.contains("== Event log\nnot recording"));
        assert!(report.contains("Controller API: browser Gamepad API"));
        assert!(report.contains(&format!("Citrine: {}", env!("CARGO_PKG_VERSION"))));
    }
}

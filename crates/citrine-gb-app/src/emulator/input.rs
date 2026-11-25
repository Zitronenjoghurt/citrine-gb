use super::Emulator;
use super::runner::Hooks;
use crate::app::ui_state::hotkeys::{HotkeyAction, Hotkeys};
use crate::app::ui_state::keybinds::{
    BindingCapture, BindingKind, Fire, GameButton, GbButton, KeyBindings,
};
use crate::combo::{Bound, Edge, Mods};
use crate::controller_debug::PadLog;
use crate::held::{Change, InputSource};
use crate::local_link::{Game, Source};
use crate::pad_input::{PadAction, PadAxis, PadEvent, PadInput, PadSource, PadTable};
use crate::recorder::InputRecorder;
use citrine_gb::console::{Console, ConsoleKind, Keys};
use strum::IntoEnumIterator;

const STICK_TILT_DEADZONE: f32 = 0.15;

pub type KeyFire = Fire<(Game, GameButton)>;

impl Emulator {
    pub fn press(&mut self, source: InputSource, button: Keys) -> bool {
        self.set_source(source, button, button)
    }

    pub fn release(&mut self, source: InputSource, button: Keys) -> bool {
        self.set_source(source, button, Keys::empty())
    }

    pub fn set_held(&mut self, source: InputSource, button: Keys, pressed: bool) -> bool {
        if pressed {
            self.press(source, button)
        } else {
            self.release(source, button)
        }
    }

    pub fn lid_closed(&self) -> bool {
        self.held.of(InputSource::Lid).contains(Keys::LID)
    }

    pub fn lid(&self) -> Option<bool> {
        (self.played_kind() == ConsoleKind::Nds).then(|| self.lid_closed())
    }

    pub fn toggle_lid(&mut self) {
        let Some(closed) = self.lid() else {
            return;
        };
        self.set_held(InputSource::Lid, Keys::LID, !closed);
    }

    pub fn held_buttons(&self) -> Keys {
        self.held.held()
    }

    pub fn is_held(&self, button: Keys) -> bool {
        self.held.held().contains(button)
    }

    pub fn take_pressed_by(&mut self) -> Option<InputSource> {
        self.pressed_by.take()
    }

    pub fn hold(&mut self, source: InputSource, down: Keys) -> bool {
        self.set_source(source, Keys::all(), down)
    }

    pub fn release_keyboard(&mut self) {
        self.key_chords.clear();
        let all = Keys::all();
        let none = Keys::empty();
        self.set_game_source(Game::First, InputSource::Keyboard, all, none);
        if self.linked() {
            self.set_game_source(Game::Second, InputSource::Keyboard, all, none);
        }
    }

    pub fn release_all(&mut self) {
        self.key_chords.clear();
        self.pad_chords.clear();
        let change = self.held.clear();
        self.apply(change);
    }

    pub(super) fn set_source(&mut self, source: InputSource, mask: Keys, down: Keys) -> bool {
        let routed = match source {
            InputSource::PadButtons(pad) | InputSource::PadAxis(pad, _) => Source::Pad(pad),
            InputSource::Touch => Source::Touch,
            InputSource::Keyboard | InputSource::Surface | InputSource::Lid => {
                return self.set_game_source(Game::First, source, mask, down);
            }
        };
        let game = self
            .link_mut()
            .map_or(Game::First, |link| link.game_for(routed));
        self.set_game_source(game, source, mask, down)
    }

    pub(super) fn set_game_source(
        &mut self,
        game: Game,
        source: InputSource,
        mask: Keys,
        down: Keys,
    ) -> bool {
        if !(down & mask).is_empty() {
            self.pressed_by = Some(source);
        }
        if game == Game::Second
            && let Some(link) = &mut self.seat.core_mut().link
        {
            let before = link.held();
            link.set_held(source, mask, down);
            return before.bits() != link.held().bits();
        }
        let before = self.held.held();
        let change = self.held.set(source, mask, down);
        self.apply(change);
        before.bits() != self.held.held().bits()
    }

    pub(super) fn apply(&mut self, change: Change) {
        super::runner::apply(
            &mut self.seat.core_mut().machine,
            change.pressed,
            change.released,
            &mut Recording(&mut self.recorder),
        );
    }

    pub(super) fn sync_joypad(&mut self) {
        let owed = self.held.take_owed();
        self.apply(Change {
            pressed: Keys::empty(),
            released: owed,
        });
        let held = self.held_buttons();
        self.seat.core_mut().machine.set_keys(held);
    }

    pub(super) fn apply_pad(
        &mut self,
        source: InputSource,
        action: PadAction,
        capture: &mut BindingCapture,
    ) -> bool {
        match action {
            PadAction::Press(button) => self.press(source, button.key()),
            PadAction::Release(button) => self.release(source, button.key()),
            PadAction::Shoulder(shoulder, true) => self.press(source, shoulder.key()),
            PadAction::Shoulder(shoulder, false) => self.release(source, shoulder.key()),
            PadAction::Ds(button, true) => self.press(source, button.key()),
            PadAction::Ds(button, false) => self.release(source, button.key()),
            PadAction::Steer { on, off } => {
                let mask = off
                    .iter()
                    .chain(on.iter())
                    .fold(Keys::empty(), |all, b| all | b.key());
                let down = on.map_or(Keys::empty(), GbButton::key);
                self.set_source(source, mask, down)
            }
            PadAction::Capture(pad, true) => {
                capture.press_pad(pad);
                true
            }
            PadAction::Capture(pad, false) => capture.release_pad(pad),
            PadAction::Unplugged => {
                let before = self.held.held();
                let change = match source {
                    InputSource::PadButtons(pad) => {
                        if let Some(link) = &mut self.seat.core_mut().link {
                            link.forget_pad(pad);
                        }
                        self.held.forget_pad(pad)
                    }
                    _ => Change::default(),
                };
                self.apply(change);
                before.bits() != self.held.held().bits()
            }
            PadAction::Button(..) | PadAction::Shortcut(..) | PadAction::Ignore(_) => false,
        }
    }

    pub fn set_hotkeys(&mut self, hotkeys: &Hotkeys) {
        if self.hotkeys != *hotkeys {
            self.hotkeys = hotkeys.clone();
        }
    }

    pub fn take_shortcuts(&mut self) -> Vec<HotkeyAction> {
        std::mem::take(&mut self.shortcuts)
    }

    pub fn holds_shortcut(&self, action: HotkeyAction) -> bool {
        self.key_chords.holds(Fire::Shortcut(action))
            || self.pad_chords.holds(Fire::Shortcut(action))
    }

    fn shortcut(&mut self, action: HotkeyAction, capture: &BindingCapture) {
        if !capture.is_waiting() {
            self.shortcuts.push(action);
        }
    }

    fn fire_pad(
        &mut self,
        source: InputSource,
        edges: &[Edge<Fire<GameButton>>],
        capture: &BindingCapture,
    ) -> bool {
        let mut changed = false;
        for edge in edges {
            changed |= match *edge {
                Edge::Down(Fire::Button(button)) => self.press(source, button.key()),
                Edge::Up(Fire::Button(button)) => self.release(source, button.key()),
                Edge::Down(Fire::Shortcut(action)) => {
                    self.shortcut(action, capture);
                    true
                }
                Edge::Up(Fire::Shortcut(_)) => true,
            };
        }
        changed
    }

    fn fire_keys(&mut self, edges: &[Edge<KeyFire>], capture: &BindingCapture) {
        for edge in edges {
            match *edge {
                Edge::Down(Fire::Button((game, button))) => {
                    let keys = button.key();
                    self.set_game_source(game, InputSource::Keyboard, keys, keys);
                }
                Edge::Up(Fire::Button((game, button))) => {
                    let keys = button.key();
                    self.set_game_source(game, InputSource::Keyboard, keys, Keys::empty());
                }
                Edge::Down(Fire::Shortcut(action)) => self.shortcut(action, capture),
                Edge::Up(Fire::Shortcut(_)) => {}
            }
        }
    }

    fn key_table(&self, bindings: &KeyBindings) -> Vec<Bound<egui::Key, KeyFire>> {
        let game: Vec<Bound<egui::Key, (Game, GameButton)>> = match self.link() {
            Some(link) => link.key_table(bindings),
            None => bindings
                .key_table(self.kind() == ConsoleKind::Nds)
                .into_iter()
                .map(|bound| bound.map(|button| (Game::First, button)))
                .collect(),
        };
        game.into_iter()
            .map(|bound| bound.map(Fire::Button))
            .chain(
                self.hotkeys
                    .key_table()
                    .into_iter()
                    .map(|bound| bound.map(Fire::Shortcut)),
            )
            .collect()
    }

    pub fn handle_pads(
        &mut self,
        pads: &mut dyn PadSource,
        bindings: &KeyBindings,
        capture: &mut BindingCapture,
        mut log: Option<&mut PadLog>,
    ) {
        let ds = self.kind() == ConsoleKind::Nds;
        let mut table: Option<PadTable> = None;
        while let Some(PadEvent { pad, input, code }) = pads.next_event() {
            match input {
                PadInput::Moved(PadAxis::RightStickX, value) => {
                    self.gimmicks.stick(Some(stick_tilt(value)), None);
                }
                PadInput::Moved(PadAxis::RightStickY, value) => {
                    self.gimmicks.stick(None, Some(-stick_tilt(value)));
                }
                _ => {}
            }
            let source = match input {
                PadInput::Moved(axis, _) => InputSource::PadAxis(pad, axis),
                _ => InputSource::PadButtons(pad),
            };
            let zone = self.held_of(pad, source);
            let steering = GbButton::iter().find(|b| zone.contains(b.key()));
            let action = crate::pad_input::route(input, bindings, capture, steering);
            let (action, changed) = match action {
                PadAction::Button(button, down) => {
                    let table: &PadTable = table.get_or_insert_with(|| {
                        crate::pad_input::pad_table(bindings, &self.hotkeys, ds)
                    });
                    let edges = self.pad_chords.change(pad, button, down, table);
                    let action = crate::pad_input::resolved(button, down, &edges, table);
                    (action, self.fire_pad(source, &edges, capture))
                }
                action => (action, self.apply_pad(source, action, capture)),
            };
            if input == PadInput::Disconnected {
                self.pad_chords.forget(pad);
            }
            let Some(log) = log.as_deref_mut() else {
                continue;
            };
            let now = web_time::Instant::now();
            match input {
                PadInput::Moved(axis, value) if self.held.of(source).bits() == zone.bits() => {
                    log.record_still(now, pad, axis, value, code, action);
                }
                _ => log.record(now, pad, input, code, action, changed),
            }
        }
    }

    pub fn handle_input(
        &mut self,
        ctx: &egui::Context,
        pads: &mut dyn PadSource,
        bindings: &KeyBindings,
        capture: &mut BindingCapture,
        log: Option<&mut PadLog>,
    ) {
        self.handle_pads(pads, bindings, capture, log);

        if ctx.wants_keyboard_input() && !capture.wants(BindingKind::Keyboard) {
            #[cfg(threaded)]
            self.away_keys.clear();
            self.release_keyboard();
            return;
        }

        #[cfg(threaded)]
        let events = {
            let mut events = std::mem::take(&mut self.away_keys);
            events.extend(key_events(ctx));
            events
        };
        #[cfg(not(threaded))]
        let events = key_events(ctx);

        let mut table: Option<Vec<Bound<egui::Key, KeyFire>>> = None;
        for (key, pressed, modifiers) in events {
            if pressed && capture.wants(BindingKind::Keyboard) {
                if key != egui::Key::Escape {
                    capture.press_key(key, modifiers);
                }
                continue;
            }
            if !pressed && capture.release_key(key) {
                continue;
            }
            let table = match &mut table {
                Some(table) => table,
                None => table.insert(self.key_table(bindings)),
            };
            let edges = if pressed {
                self.key_chords.press(key, Mods::of(modifiers), table)
            } else {
                self.key_chords.release(key)
            };
            self.fire_keys(&edges, capture);
            let bound = table.iter().any(|bound| bound.combo.contains(&key));
            if !bound
                && self.cart().motion()
                && let Some(tilt) = bindings.tilt_for_key(key)
            {
                self.gimmicks.tilt(tilt, pressed);
            }
        }
    }
}

pub(super) fn key_events(ctx: &egui::Context) -> Vec<(egui::Key, bool, egui::Modifiers)> {
    ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|event| match event {
                egui::Event::Key {
                    key,
                    pressed,
                    repeat: false,
                    modifiers,
                    ..
                } => Some((*key, *pressed, *modifiers)),
                _ => None,
            })
            .collect()
    })
}

fn stick_tilt(value: f32) -> f32 {
    if value.abs() < STICK_TILT_DEADZONE {
        0.0
    } else {
        value
    }
}

struct Recording<'a>(&'a mut InputRecorder);

impl Hooks for Recording<'_> {
    fn record(&mut self, cycles: u128, key: Keys, pressed: bool) {
        self.0.record(cycles, key, pressed);
    }
}

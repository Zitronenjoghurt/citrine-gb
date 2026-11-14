use super::Emulator;
use super::runner::Hooks;
use crate::app::ui_state::keybinds::{
    BindingCapture, BindingKind, Captured, GbButton, KeyBindings, PadButton,
};
use crate::controller_debug::PadLog;
use crate::held::{Change, InputSource};
use crate::local_link::{Game, Source};
use crate::pad_input::{Ignored, PadAction, PadAxis, PadEvent, PadInput, PadKey, PadSource};
use crate::recorder::InputRecorder;
use citrine_gb::console::{Console, ConsoleKind, Keys};
use strum::IntoEnumIterator;

const STICK_TILT_DEADZONE: f32 = 0.15;

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
        let all = Keys::all();
        let none = Keys::empty();
        self.set_game_source(Game::First, InputSource::Keyboard, all, none);
        if self.linked() {
            self.set_game_source(Game::Second, InputSource::Keyboard, all, none);
        }
    }

    pub fn release_all(&mut self) {
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
            PadAction::Capture(pad) => {
                capture.record(Captured::Pad(pad));
                true
            }
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
            PadAction::Ignore(_) => false,
        }
    }

    pub fn take_loose_presses(&mut self) -> Vec<PadButton> {
        std::mem::take(&mut self.loose_presses)
    }

    pub fn loose_held(&self) -> &[PadButton] {
        &self.loose_held
    }

    pub fn handle_pads(
        &mut self,
        pads: &mut dyn PadSource,
        bindings: &KeyBindings,
        capture: &mut BindingCapture,
        mut log: Option<&mut PadLog>,
    ) {
        let ds = self.kind() == ConsoleKind::Nds;
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
            let action = crate::pad_input::route(input, bindings, capture, steering, ds);
            if let (PadInput::Pressed(PadKey::Known(button)), PadAction::Ignore(Ignored::Unbound)) =
                (input, action)
            {
                self.loose_presses.push(button);
                self.loose_held.push(button);
            }
            match input {
                PadInput::Released(PadKey::Known(button)) => {
                    self.loose_held.retain(|held| *held != button);
                }
                PadInput::Disconnected | PadInput::Dropped => self.loose_held.clear(),
                _ => {}
            }
            let changed = self.apply_pad(source, action, capture);
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

        for (key, pressed, modifiers) in events {
            if capture.wants(BindingKind::Keyboard) {
                if pressed && key != egui::Key::Escape {
                    capture.record(Captured::Key(key, modifiers));
                }
                continue;
            }
            let routed = match self.link() {
                Some(link) => link
                    .key(key, bindings)
                    .map(|(game, button)| (game, button.key())),
                None => bindings
                    .input_for_key(key, self.kind() == ConsoleKind::Nds)
                    .map(|keys| (Game::First, keys)),
            };
            if let Some((game, joypad)) = routed {
                let down = if pressed { joypad } else { Keys::empty() };
                self.set_game_source(game, InputSource::Keyboard, joypad, down);
            } else if self.cart().motion() {
                self.gimmicks.tilt_key(key, pressed);
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

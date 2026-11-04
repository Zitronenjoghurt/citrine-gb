use super::Emulator;
use crate::app::ui_state::keybinds::{
    BindingCapture, BindingKind, Captured, GbButton, KeyBindings, PadButton, Shoulder,
};
use crate::controller_debug::PadLog;
use crate::held::{Change, InputSource};
use crate::local_link::{Game, Source};
use crate::pad_input::{Ignored, PadAction, PadAxis, PadEvent, PadInput, PadKey, PadSource};
use citrine_gb::console::{Console, Keys};
use citrine_gb::debug::Debug;
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

    pub fn held_buttons(&self) -> Keys {
        self.held.held()
    }

    pub fn is_held(&self, button: Keys) -> bool {
        self.held.held().contains(button)
    }

    pub fn take_pressed_by(&mut self) -> Option<InputSource> {
        self.pressed_by.take()
    }

    pub fn release_keyboard(&mut self) {
        let all = Keys::all();
        let none = Keys::empty();
        self.set_game_source(Game::First, InputSource::Keyboard, all, none);
        if self.link.is_some() {
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
            InputSource::Keyboard => return self.set_game_source(Game::First, source, mask, down),
        };
        let game = self
            .link
            .as_mut()
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
            && let Some(link) = &mut self.link
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
        let total_cycles = Debug::cycles(&self.machine);
        let buttons = GbButton::iter().map(GbButton::key);
        for joypad in buttons.chain(Shoulder::iter().map(Shoulder::key)) {
            if change.pressed.contains(joypad) {
                self.machine.set_keys(self.machine.keys() | joypad);
                self.recorder.record(total_cycles, joypad, true);
            } else if change.released.contains(joypad) {
                self.machine.set_keys(self.machine.keys() - joypad);
                self.recorder.record(total_cycles, joypad, false);
            }
        }
    }

    pub(super) fn sync_joypad(&mut self) {
        let owed = self.held.take_owed();
        self.apply(Change {
            pressed: Keys::empty(),
            released: owed,
        });
        self.machine.set_keys(self.held_buttons());
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
                        if let Some(link) = &mut self.link {
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

    pub fn handle_pads(
        &mut self,
        pads: &mut dyn PadSource,
        bindings: &KeyBindings,
        capture: &mut BindingCapture,
        mut log: Option<&mut PadLog>,
    ) {
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
            if let (PadInput::Pressed(PadKey::Known(button)), PadAction::Ignore(Ignored::Unbound)) =
                (input, action)
            {
                self.loose_presses.push(button);
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
            self.release_keyboard();
            return;
        }

        let events: Vec<(egui::Key, bool, egui::Modifiers)> = ctx.input(|i| {
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
        });

        for (key, pressed, modifiers) in events {
            if capture.wants(BindingKind::Keyboard) {
                if pressed && key != egui::Key::Escape {
                    capture.record(Captured::Key(key, modifiers));
                }
                continue;
            }
            let routed = match &self.link {
                Some(link) => link
                    .key(key, bindings)
                    .map(|(game, button)| (game, button.key())),
                None => bindings.input_for_key(key).map(|keys| (Game::First, keys)),
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

fn stick_tilt(value: f32) -> f32 {
    if value.abs() < STICK_TILT_DEADZONE {
        0.0
    } else {
        value
    }
}

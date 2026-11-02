use super::Emulator;
use crate::app::ui_state::keybinds::{
    BindingCapture, BindingKind, Captured, GbButton, KeyBindings, PadButton,
};
use crate::controller_debug::PadLog;
use crate::held::{Change, InputSource};
use crate::local_link::{Game, Source};
use crate::pad_input::{Ignored, PadAction, PadEvent, PadInput, PadKey, PadSource};
use citrine_gb::gb::joypad::JoypadState;
use strum::IntoEnumIterator;

impl Emulator {
    pub fn press(&mut self, source: InputSource, button: JoypadState) -> bool {
        self.set_source(source, button, button)
    }

    pub fn release(&mut self, source: InputSource, button: JoypadState) -> bool {
        self.set_source(source, button, JoypadState::empty())
    }

    pub fn set_held(&mut self, source: InputSource, button: JoypadState, pressed: bool) -> bool {
        if pressed {
            self.press(source, button)
        } else {
            self.release(source, button)
        }
    }

    pub fn held_buttons(&self) -> JoypadState {
        self.held.held()
    }

    pub fn is_held(&self, button: JoypadState) -> bool {
        self.held.held().contains(button)
    }

    pub fn release_all(&mut self) {
        let change = self.held.clear();
        self.apply(change);
    }

    pub(super) fn set_source(
        &mut self,
        source: InputSource,
        mask: JoypadState,
        down: JoypadState,
    ) -> bool {
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
        mask: JoypadState,
        down: JoypadState,
    ) -> bool {
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
        let total_cycles = self.gb.debugger.total_cycles;
        for button in GbButton::iter() {
            let joypad = button.joypad();
            if change.pressed.contains(joypad) {
                self.gb.press_button(joypad);
                self.recorder.record(total_cycles, joypad, true);
            } else if change.released.contains(joypad) {
                self.gb.release_button(joypad);
                self.recorder.record(total_cycles, joypad, false);
            }
        }
    }

    pub(super) fn sync_joypad(&mut self) {
        let owed = self.held.take_owed();
        self.apply(Change {
            pressed: JoypadState::empty(),
            released: owed,
        });
        for button in GbButton::iter() {
            let joypad = button.joypad();
            if self.is_held(joypad) {
                self.gb.press_button(joypad);
            } else {
                self.gb.release_button(joypad);
            }
        }
    }

    pub(super) fn apply_pad(
        &mut self,
        source: InputSource,
        action: PadAction,
        capture: &mut BindingCapture,
    ) -> bool {
        match action {
            PadAction::Press(button) => self.press(source, button.joypad()),
            PadAction::Release(button) => self.release(source, button.joypad()),
            PadAction::Steer { on, off } => {
                let mask = off
                    .iter()
                    .chain(on.iter())
                    .fold(JoypadState::empty(), |all, b| all | b.joypad());
                let down = on.map_or(JoypadState::empty(), GbButton::joypad);
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
            let source = match input {
                PadInput::Moved(axis, _) => InputSource::PadAxis(pad, axis),
                _ => InputSource::PadButtons(pad),
            };
            let zone = self.held_of(pad, source);
            let steering = GbButton::iter().find(|b| zone.contains(b.joypad()));
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
            return;
        }

        let events: Vec<(egui::Key, bool, egui::Modifiers)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::Key {
                        key,
                        pressed,
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
                Some(link) => link.key(key, bindings),
                None => bindings
                    .button_for_key(key)
                    .map(|button| (Game::First, button)),
            };
            if let Some((game, button)) = routed {
                let joypad = button.joypad();
                let down = if pressed {
                    joypad
                } else {
                    JoypadState::empty()
                };
                self.set_game_source(game, InputSource::Keyboard, joypad, down);
            }
        }
    }
}

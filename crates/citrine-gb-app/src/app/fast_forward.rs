use super::Citrine;
use crate::app::ui_state::hotkeys::{HotkeyAction, Hotkeys};
use crate::emulator::fast_forward::FastForwardSpeed;
use egui::Context;

#[derive(Debug, Default)]
pub(super) struct FastForwardInput {
    clicked: bool,
    key: bool,
    held: bool,
}

impl Citrine {
    pub(super) fn fast_forward_key(&mut self, ctx: &Context, raw: &mut egui::RawInput) {
        let typing = ctx.wants_keyboard_input() || self.ui.capture.is_waiting();
        if typing || !self.emulator.has_game() || self.ui.viewer.is_open() {
            self.fast_forward.key = false;
            return;
        }
        let keys = self.ui.settings.hotkeys.keys_for(HotkeyAction::FastForward);
        let down = &mut self.fast_forward.key;
        raw.events.retain(|event| match event {
            egui::Event::Key {
                key,
                pressed,
                modifiers,
                ..
            } if keys.iter().any(|hotkey| hotkey.key == *key) => {
                *down = *pressed && keys.iter().any(|hotkey| hotkey.matches(*key, *modifiers));
                false
            }
            egui::Event::WindowFocused(false) => {
                *down = false;
                true
            }
            _ => true,
        });
    }

    pub(super) fn follow_fast_forward(&mut self, ctx: &Context) {
        let clicked = std::mem::take(&mut self.fast_forward.clicked);
        let hotkeys = &self.ui.settings.hotkeys;
        let pad = !self.ui.capture.is_waiting()
            && self
                .emulator
                .loose_held()
                .iter()
                .any(|pad| hotkeys.action_for_pad(*pad) == Some(HotkeyAction::FastForward));
        let held = self.fast_forward.key || pad;
        if clicked {
            self.emulator.fast_forward = !self.emulator.fast_forward;
        }
        if held {
            self.emulator.fast_forward = true;
        } else if self.fast_forward.held {
            self.emulator.fast_forward = false;
        }
        self.fast_forward.held = held;
        let rewinding = self.emulator.rewind_held && self.emulator.can_rewind();
        if rewinding || self.fast_forward_off_reason().is_some() {
            self.emulator.fast_forward = false;
        }
        if self.emulator.fast_forward {
            ctx.request_repaint();
        }
    }

    pub(super) fn fast_forward_off_reason(&self) -> Option<&'static str> {
        if !self.emulator.has_game() {
            Some("Open a game to fast forward it")
        } else if self.play.session().is_some() {
            Some("Fast forward is off while playing together")
        } else if self.emulator.screen_recorder.is_recording() {
            Some("Fast forward is off while recording")
        } else {
            None
        }
    }

    pub(super) fn fast_forward_button(&mut self, ui: &mut egui::Ui) {
        let action = HotkeyAction::FastForward;
        let button = egui::Button::new(action.icon(false)).selected(self.emulator.fast_forward);
        let off = self.fast_forward_off_reason();
        let hint = fast_forward_hint(&self.ui.settings.hotkeys, self.emulator.fast_forward_speed);
        let response = ui
            .add_enabled(off.is_none(), button)
            .on_hover_text(hint)
            .on_disabled_hover_text(off.unwrap_or_default());
        self.fast_forward.clicked |= response.clicked();
    }
}

fn fast_forward_hint(hotkeys: &Hotkeys, speed: FastForwardSpeed) -> String {
    match hotkeys.key_name(HotkeyAction::FastForward) {
        Some(key) => format!("Fast forward ({speed}, hold {key})"),
        None => format!("Fast forward ({speed})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::{phone_app, tap, text_rect};
    use crate::app::ui_state::keybinds::GbButton;
    use crate::app::ui_state::settings::LayoutMode;
    use citrine_gb::gb::GbModel;
    use citrine_gb::rom::Rom;

    fn desktop(name: &str) -> Citrine {
        let mut app = phone_app(name);
        app.ui.settings.layout = LayoutMode::Desktop;
        app
    }

    fn playing(name: &str) -> Citrine {
        let mut app = desktop(name);
        app.emulator
            .load_rom(
                &Rom::new(&[0u8; 0x8000]),
                GbModel::Dmg,
                #[cfg(not(target_arch = "wasm32"))]
                None,
            )
            .expect("load");
        app.emulator.running = true;
        app
    }

    fn frame(app: &mut Citrine, ctx: &Context, events: Vec<egui::Event>) -> egui::FullOutput {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            events,
            ..Default::default()
        };
        app.raw_keys(ctx, &mut input);
        ctx.run(input, |ctx| {
            crate::app::mobile::touch_spacing(ctx, false);
            app.render(ctx);
            app.handle_snapshot_hotkeys(ctx);
            let Citrine {
                emulator, pads, ui, ..
            } = app;
            emulator.handle_input(
                ctx,
                pads.as_mut(),
                &ui.settings.bindings,
                &mut ui.capture,
                None,
            );
            app.follow_rewind(ctx);
            app.follow_fast_forward(ctx);
        })
    }

    fn key(key: egui::Key, pressed: bool) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Default::default(),
        }
    }

    fn button(app: &mut Citrine, ctx: &Context) -> egui::Pos2 {
        let out = frame(app, ctx, vec![]);
        text_rect(&out, crate::icons::FAST_FORWARD)
            .expect("fast forward button")
            .center()
    }

    fn hover(app: &mut Citrine, ctx: &Context, at: egui::Pos2) -> egui::FullOutput {
        frame(
            app,
            ctx,
            vec![egui::Event::PointerMoved(at + egui::vec2(0.0, 200.0))],
        );
        for _ in 0..12 {
            frame(app, ctx, vec![]);
        }
        frame(app, ctx, vec![egui::Event::PointerMoved(at)]);
        let mut out = frame(app, ctx, vec![]);
        for _ in 0..60 {
            out = frame(app, ctx, vec![]);
        }
        out
    }

    #[test]
    fn holding_space_fast_forwards_and_letting_go_stops_it() {
        let mut app = playing("ff-hold");
        let ctx = Context::default();
        frame(&mut app, &ctx, vec![]);
        assert!(!app.emulator.fast_forward);
        frame(&mut app, &ctx, vec![key(egui::Key::Space, true)]);
        assert!(app.emulator.fast_forward);
        frame(&mut app, &ctx, vec![]);
        assert!(app.emulator.fast_forward, "still held");
        frame(&mut app, &ctx, vec![key(egui::Key::Space, false)]);
        assert!(!app.emulator.fast_forward, "off in the frame it is let go");
    }

    #[test]
    fn space_drives_only_fast_forward_while_a_game_runs() {
        let mut app = playing("ff-alone");
        let ctx = Context::default();
        frame(&mut app, &ctx, vec![]);
        frame(&mut app, &ctx, vec![key(egui::Key::Space, true)]);
        assert!(!app.emulator.is_held(GbButton::Start.key()));
        assert_eq!(ctx.memory(|m| m.focused()), None);
        frame(&mut app, &ctx, vec![key(egui::Key::Space, false)]);
        frame(&mut app, &ctx, vec![key(egui::Key::Enter, true)]);
        assert!(app.emulator.is_held(GbButton::Start.key()));
    }

    #[test]
    fn space_never_presses_a_focused_toolbar_button() {
        let mut app = playing("ff-focus");
        let ctx = Context::default();
        frame(&mut app, &ctx, vec![]);
        frame(&mut app, &ctx, vec![key(egui::Key::Tab, true)]);
        frame(&mut app, &ctx, vec![key(egui::Key::Tab, false)]);
        let focused = ctx.memory(|m| m.focused());
        let running = app.emulator.running;
        for pressed in [true, false] {
            frame(&mut app, &ctx, vec![key(egui::Key::Space, pressed)]);
        }
        assert_eq!(app.emulator.running, running);
        assert_eq!(ctx.memory(|m| m.focused()), focused);
    }

    #[test]
    fn a_rebound_key_fast_forwards_and_space_goes_back_to_the_game() {
        use crate::app::ui_state::hotkeys::Hotkey;
        let mut app = playing("ff-rebound");
        let hotkeys = &mut app.ui.settings.hotkeys;
        hotkeys.unbind(Hotkey::plain(egui::Key::Space));
        hotkeys.bind(Hotkey::plain(egui::Key::F), HotkeyAction::FastForward);
        app.ui
            .settings
            .bindings
            .bind_key(egui::Key::Space, GbButton::Start);
        let ctx = Context::default();
        frame(&mut app, &ctx, vec![]);
        frame(&mut app, &ctx, vec![key(egui::Key::Space, true)]);
        assert!(!app.emulator.fast_forward);
        assert!(app.emulator.is_held(GbButton::Start.key()));
        frame(&mut app, &ctx, vec![key(egui::Key::F, true)]);
        assert!(app.emulator.fast_forward);
        frame(&mut app, &ctx, vec![key(egui::Key::F, false)]);
        assert!(!app.emulator.fast_forward);
    }

    #[test]
    fn the_toolbar_button_turns_fast_forward_on_and_off() {
        let mut app = playing("ff-button");
        let ctx = Context::default();
        let at = button(&mut app, &ctx);
        frame(&mut app, &ctx, tap(at));
        assert!(app.emulator.fast_forward);
        frame(&mut app, &ctx, vec![]);
        assert!(app.emulator.fast_forward, "stays on without a key held");
        frame(&mut app, &ctx, tap(at));
        assert!(!app.emulator.fast_forward);
    }

    #[test]
    fn the_toolbar_button_names_the_speed_and_the_key() {
        let mut app = playing("ff-hint");
        app.emulator.fast_forward_speed = FastForwardSpeed::Five;
        let ctx = Context::default();
        let at = button(&mut app, &ctx);
        let out = hover(&mut app, &ctx, at);
        assert!(text_rect(&out, "Fast forward (5x, hold Space)").is_some());
    }

    #[test]
    fn without_a_game_the_button_does_nothing_and_says_why() {
        let mut app = desktop("ff-no-game");
        let ctx = Context::default();
        let at = button(&mut app, &ctx);
        frame(&mut app, &ctx, tap(at));
        frame(&mut app, &ctx, vec![key(egui::Key::Space, true)]);
        assert!(!app.emulator.fast_forward);
        let out = hover(&mut app, &ctx, at);
        assert!(text_rect(&out, "Open a game to fast forward it").is_some());
    }

    #[test]
    fn rewinding_turns_fast_forward_off() {
        let mut app = playing("ff-rewind");
        let ctx = Context::default();
        let at = button(&mut app, &ctx);
        frame(&mut app, &ctx, tap(at));
        assert!(app.emulator.fast_forward);
        frame(&mut app, &ctx, vec![key(egui::Key::R, true)]);
        assert!(!app.emulator.fast_forward);
        frame(&mut app, &ctx, vec![key(egui::Key::R, false)]);
        assert!(!app.emulator.fast_forward);
    }
}

use super::Citrine;
use super::events::AppEvent;
use super::ui_state::hotkeys::HotkeyAction;
use super::ui_state::screen_layouts::Item;
use super::ui_state::settings_tree::rows::RowId;
use crate::audio::mic::{Microphone, Wanted};
use citrine_gb::console::ConsoleKind;
use egui::Context;
use std::hash::{Hash, Hasher};

impl Citrine {
    pub(crate) fn talk_button_wanted(&self) -> bool {
        let s = &self.ui.settings;
        s.push_to_talk
            && s.talk_button
            && s.microphone != Microphone::Off
            && self.emulator.kind() == ConsoleKind::Nds
            && self.emulator.has_game()
    }

    pub(crate) fn mic_mark_offered(&self) -> bool {
        self.ui.settings.microphone != Microphone::Off
            && self.emulator.kind() == ConsoleKind::Nds
            && self.emulator.has_game()
    }

    pub(crate) fn mic_live(&self) -> bool {
        self.mic.is_live()
    }

    fn talking(&mut self, ctx: &Context) -> bool {
        let pressed = std::mem::take(&mut self.talk_pressed);
        let typing = ctx.wants_keyboard_input() || self.ui.capture.is_waiting();
        let button = self.mobile.held_buttons.contains(&Item::Talk);
        pressed || button || (!typing && self.emulator.holds_shortcut(HotkeyAction::PushToTalk))
    }

    pub(super) fn follow_mic(&mut self, ctx: &Context) {
        let talking = self.talking(ctx);
        let ds = self.emulator.kind() == ConsoleKind::Nds && self.emulator.has_game();
        if ds {
            self.emulator.attach_mic(self.mic.line());
        }
        let engine = self
            .play
            .session()
            .is_some_and(|session| session.runs_engine());
        let wanted = Wanted {
            game: ds.then(|| game_id(self.emulator.rom_key())),
            running: self.emulator.is_running() && !engine,
            in_use: ds && self.emulator.mic_in_use(),
            push_to_talk: self.ui.settings.push_to_talk,
            talking,
            testing: std::mem::take(&mut self.ui.settings.mic_test_shown),
        };
        let live = self.mic.is_live();
        let notice = self.mic.follow_platform(
            &self.ui.settings.microphone,
            wanted,
            web_time::Instant::now(),
        );
        if live != self.mic.is_live() {
            ctx.request_repaint();
        }
        if self.mic.is_open() {
            ctx.request_repaint_after(POLL);
        }
        if self.mic.test().is_some() {
            ctx.request_repaint_after(METER);
        }
        let Some(notice) = notice else {
            return;
        };
        if wanted.testing && notice.needs_action() {
            return;
        }
        if notice.needs_action() {
            self.toasts
                .error(notice.text())
                .button(
                    "Settings",
                    AppEvent::FindSetting {
                        row: RowId::Microphone,
                    },
                )
                .needs_action();
        } else {
            self.toasts.info(notice.text());
        }
    }
}

const POLL: std::time::Duration = std::time::Duration::from_millis(250);
const METER: std::time::Duration = std::time::Duration::from_millis(33);

fn game_id(key: Option<&str>) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::phone_app;

    fn ds_app(name: &str) -> Citrine {
        let mut app = phone_app(name);
        app.emulator
            .load_nds(include_bytes!(
                "../../fixtures/nds/blocksds-console-both-screens.nds"
            ))
            .unwrap();
        app
    }

    #[test]
    fn the_talk_button_shows_only_on_a_ds_game_with_push_to_talk() {
        let mut app = phone_app("mic-talk-none");
        app.ui.settings.microphone = Microphone::Default;
        app.ui.settings.push_to_talk = true;
        assert!(app.wanted_buttons().is_empty());
        let mut app = ds_app("mic-talk-ds");
        assert!(app.wanted_buttons().is_empty());
        app.ui.settings.microphone = Microphone::Default;
        assert!(app.wanted_buttons().is_empty());
        app.ui.settings.push_to_talk = true;
        assert_eq!(
            app.wanted_buttons(),
            vec![(Item::Talk, crate::icons::MICROPHONE)]
        );
        app.ui.settings.talk_button = false;
        assert!(app.wanted_buttons().is_empty());
    }

    #[test]
    fn the_mic_mark_waits_in_edit_mode_once_the_microphone_is_on() {
        let mut app = ds_app("mic-mark-edit");
        app.ui.settings.microphone = Microphone::Off;
        assert!(!app.mic_mark_offered());
        app.ui.settings.microphone = Microphone::Default;
        assert!(app.mic_mark_offered());
        assert!(!app.mic_live());
    }

    #[test]
    fn holding_the_talk_button_counts_as_talking() {
        let mut app = ds_app("mic-talk-held");
        let ctx = Context::default();
        let talking = |app: &mut Citrine| {
            let mut out = false;
            let _ = ctx.run(egui::RawInput::default(), |ctx| out = app.talking(ctx));
            out
        };
        assert!(!talking(&mut app));
        app.mobile.held_buttons.push(Item::Talk);
        assert!(talking(&mut app));
        app.mobile.held_buttons.clear();
        assert!(!talking(&mut app));
    }
}

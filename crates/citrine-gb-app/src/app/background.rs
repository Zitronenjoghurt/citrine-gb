use super::Citrine;
use egui::Context;
use std::fmt::Display;

#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    strum_macros::EnumIter,
)]
pub enum InBackground {
    #[default]
    KeepPlaying,
    Mute,
    Pause,
}

impl Display for InBackground {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::KeepPlaying => "Play on",
            Self::Mute => "Mute",
            Self::Pause => "Pause",
        })
    }
}

#[derive(Debug, Default)]
pub(super) struct Away {
    muted: bool,
    paused: bool,
}

impl Citrine {
    pub(super) fn follow_background(&mut self, ctx: &Context, visible: bool) {
        let away = !visible
            || ctx.input(|i| {
                let viewport = i.viewport();
                viewport.focused == Some(false) || viewport.minimized == Some(true)
            });
        let choice = if away {
            self.ui.settings.background_pause
        } else {
            InBackground::KeepPlaying
        };
        let together = self.play.session().is_some() || self.play.room().is_some();
        let pause = choice == InBackground::Pause && !together;
        if pause && !self.away.paused && self.emulator.running {
            self.emulator.toggle_running();
            self.away.paused = true;
        } else if !pause && std::mem::take(&mut self.away.paused) && !self.emulator.running {
            self.emulator.toggle_running();
        }
        if choice != InBackground::KeepPlaying {
            self.emulator.sound.mine = 0.0;
            self.emulator.sound.friend = 0.0;
            self.away.muted = true;
        } else if std::mem::take(&mut self.away.muted) {
            self.emulator.sound = self.ui.settings.sound_levels();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::phone_app;
    use citrine_gb::gb::GbModel;
    use citrine_gb::rom::Rom;

    fn playing(name: &str) -> Citrine {
        let mut app = phone_app(name);
        app.emulator
            .load_rom(
                &Rom::new(&[0u8; 0x8000]),
                GbModel::Dmg,
                #[cfg(not(target_arch = "wasm32"))]
                None,
            )
            .expect("load");
        app.emulator.running = true;
        app.emulator.sound = app.ui.settings.sound_levels();
        app
    }

    fn frame(app: &mut Citrine, ctx: &Context, focused: bool) {
        let mut input = egui::RawInput::default();
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .focused = Some(focused);
        let _ = ctx.run(input, |ctx| app.follow_background(ctx, true));
    }

    #[test]
    fn by_default_the_game_keeps_playing_and_sounding_in_the_background() {
        let mut app = playing("background-keep");
        let ctx = Context::default();
        let sound = app.emulator.sound;
        frame(&mut app, &ctx, false);
        assert!(app.emulator.running);
        assert_eq!(app.emulator.sound, sound);
    }

    #[test]
    fn mute_silences_the_game_until_the_window_is_back() {
        let mut app = playing("background-mute");
        app.ui.settings.background_pause = InBackground::Mute;
        let ctx = Context::default();
        let sound = app.emulator.sound;
        assert!(sound.mine > 0.0);
        frame(&mut app, &ctx, false);
        assert!(app.emulator.running, "mute keeps the game running");
        assert_eq!(app.emulator.sound.mine, 0.0);
        frame(&mut app, &ctx, true);
        assert_eq!(app.emulator.sound, sound);
        assert!(!app.ui.settings.muted, "the mute setting is left alone");
    }

    #[test]
    fn pause_stops_the_game_and_resumes_only_what_it_paused() {
        let mut app = playing("background-pause");
        app.ui.settings.background_pause = InBackground::Pause;
        let ctx = Context::default();
        frame(&mut app, &ctx, false);
        assert!(!app.emulator.running);
        assert_eq!(app.emulator.sound.mine, 0.0);
        frame(&mut app, &ctx, false);
        assert!(!app.emulator.running);
        frame(&mut app, &ctx, true);
        assert!(app.emulator.running, "back in front, the game goes on");

        app.emulator.toggle_running();
        frame(&mut app, &ctx, false);
        frame(&mut app, &ctx, true);
        assert!(!app.emulator.running, "a game paused by hand stays paused");
    }

    #[test]
    fn the_background_choice_stays_on_this_device() {
        use crate::app::ui_state::synced::synced_values;
        let mut settings = crate::app::ui_state::settings::Settings::default();
        assert_eq!(settings.background_pause, InBackground::KeepPlaying);
        settings.background_pause = InBackground::Pause;
        assert!(!synced_values(&settings).contains_key("background_pause"));
        assert_eq!(InBackground::KeepPlaying.to_string(), "Play on");
    }
}

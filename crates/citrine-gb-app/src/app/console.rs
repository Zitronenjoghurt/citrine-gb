use super::widgets;
use super::{Citrine, PendingConsole};
use crate::app::ui_state::settings::{console_name, plays_on_both};
use citrine_gb::gb::GbModel;
use egui::Context;

impl Citrine {
    pub(super) fn ask_play_on(&mut self, model: GbModel) {
        if self.emulator.rom_key().is_none()
            || self.emulator.console() == model
            || !self
                .emulator
                .as_gb()
                .is_some_and(|gb| plays_on_both(gb.cartridge.header.cgb_mode))
        {
            return;
        }
        if !self.emulator.has_been_playing() {
            self.play_on(model);
            return;
        }
        if self.pending_console.is_none() {
            self.pending_console = Some(PendingConsole {
                model,
                rom_key: self.emulator.rom_key().map(str::to_owned),
                was_running: self.emulator.running,
            });
            self.emulator.running = false;
            self.emulator.release_all();
        }
    }

    pub(super) fn console_confirm(&mut self, ctx: &Context) {
        let Some(pending) = &self.pending_console else {
            return;
        };
        if pending.rom_key.as_deref() != self.emulator.rom_key() {
            self.pending_console = None;
            return;
        }
        let choice = widgets::confirm::confirm(
            ctx,
            "console_confirm",
            &format!("Restart the game on {}?", console_name(pending.model)),
            "Restart",
        );
        if let Some(confirmed) = choice {
            self.finish_play_on(confirmed);
        }
    }

    pub(super) fn finish_play_on(&mut self, confirmed: bool) {
        let Some(pending) = self.pending_console.take() else {
            return;
        };
        if confirmed {
            self.play_on(pending.model);
        } else {
            self.emulator.running = pending.was_running;
        }
    }

    pub(super) fn play_on(&mut self, model: GbModel) {
        let Some(key) = self.emulator.rom_key().map(str::to_owned) else {
            return;
        };
        self.ui.settings.set_game_console(&key, model);
        match self.emulator.restart_on(model) {
            Ok(()) => {
                self.toasts
                    .success(format!("Restarted on {}", console_name(model)));
            }
            Err(err) => {
                self.toasts
                    .error(format!("Could not restart the game: {err:?}"));
            }
        }
        self.ui.settings.dirty = true;
        self.ui.snapshots.invalidate();
    }

    pub(super) fn note_console_switch(&mut self) {
        if let Some(model) = self.emulator.take_console_switch() {
            self.toasts.success(format!(
                "Switched to {}, where this save was made",
                console_name(model)
            ));
            self.ui.settings.dirty = true;
        }
    }
}

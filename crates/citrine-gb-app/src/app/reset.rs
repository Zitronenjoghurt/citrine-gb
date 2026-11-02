use super::widgets;
use super::{Citrine, PendingReset, RESET_STOP_ASKING};
use egui::Context;

impl Citrine {
    pub(super) fn ask_reset(&mut self) {
        if self.reset_without_asking {
            self.reset_game();
            return;
        }
        if self.pending_reset.is_none() {
            self.pending_reset = Some(PendingReset {
                was_running: self.emulator.running,
                stop_asking: false,
            });
            self.emulator.running = false;
            self.emulator.release_all();
        }
    }

    pub(super) fn reset_game(&mut self) {
        self.emulator.restart();
        self.toasts.success("Game reset");
    }

    pub(super) fn reset_confirm(&mut self, ctx: &Context) {
        let Some(pending) = &mut self.pending_reset else {
            return;
        };
        if self.emulator.rom_key().is_none() {
            self.pending_reset = None;
            return;
        }
        let choice = widgets::confirm::confirm_with_option(
            ctx,
            "reset_confirm",
            "Reset the game?",
            "Reset",
            (RESET_STOP_ASKING, &mut pending.stop_asking),
        );
        if let Some(confirmed) = choice {
            self.finish_reset(confirmed);
        }
    }

    pub(super) fn finish_reset(&mut self, confirmed: bool) {
        let Some(pending) = self.pending_reset.take() else {
            return;
        };
        self.emulator.running = pending.was_running;
        if confirmed {
            self.reset_without_asking = pending.stop_asking;
            self.reset_game();
        }
    }
}

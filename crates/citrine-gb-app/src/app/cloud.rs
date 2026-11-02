use super::widgets;
use super::{CLOUD_HOLD_LIMIT, CLOUD_HOLD_TOAST, Citrine, CloudHold};
use egui::Context;

impl Citrine {
    pub(super) fn watch_cloud_game(&mut self) {
        let rom = self.emulator.rom_key().map(str::to_owned);
        if rom == self.cloud_rom {
            return;
        }
        self.cloud_rom = rom.clone();
        let title = self.emulator.game_name().to_owned();
        self.online.cloud_game(rom.clone().map(|rom| (rom, title)));
        let Some(rom) = rom else {
            return;
        };
        if !self.online.cloud_enabled()
            || (self.online.cloud_settled(&rom) && !self.online.is_connected())
        {
            return;
        }
        self.online.cloud_hold(&rom);
        let was_running = match self.cloud_hold.take() {
            Some(hold) => hold.was_running,
            None => self.emulator.running,
        };
        self.cloud_hold = Some(CloudHold {
            since: web_time::Instant::now(),
            was_running,
            told: false,
        });
        self.emulator.running = false;
    }

    pub(super) fn cloud_conflict(&mut self, ctx: &Context) {
        let Some(conflict) = self.online.cloud_ask() else {
            return;
        };
        let answer = widgets::cloud_conflict::show(ctx, conflict);
        let (rom, slot) = (conflict.rom.clone(), conflict.slot.clone());
        match answer {
            None => {}
            Some(widgets::cloud_conflict::Answer::NotNow) => self.online.cloud_dismiss(&rom, &slot),
            Some(widgets::cloud_conflict::Answer::Choose(choice)) => {
                if let Err(err) = self.online.cloud_resolve(&rom, &slot, choice) {
                    self.toasts
                        .error(format!("Could not keep that copy: {err}"));
                }
                self.ui.snapshots.invalidate();
            }
        }
    }

    pub(super) fn settle_cloud_game(&mut self, now: web_time::Instant) {
        if let Some(hold) = &mut self.cloud_hold {
            let settled = self
                .cloud_rom
                .as_deref()
                .is_none_or(|rom| self.online.cloud_settled(rom));
            let waited = now.duration_since(hold.since);
            if !settled && waited < CLOUD_HOLD_LIMIT {
                if waited >= CLOUD_HOLD_TOAST && !hold.told {
                    hold.told = true;
                    self.toasts.info("Getting your save from the cloud");
                }
                return;
            }
            let was_running = hold.was_running;
            self.cloud_hold = None;
            self.online.cloud_release();
            if self.online.cloud_take_reload()
                && let Err(err) = self.emulator.reload_battery()
            {
                self.toasts
                    .error(format!("Could not load the cloud save: {err}"));
            }
            self.emulator.running = was_running;
            return;
        }
        if self.online.cloud_take_reload() {
            match self.emulator.reload_battery() {
                Ok(()) => {
                    self.toasts.success("Loaded the save from the cloud");
                }
                Err(err) => {
                    self.toasts
                        .error(format!("Could not load the cloud save: {err}"));
                }
            }
        }
        if self.online.cloud_take_changed() {
            self.ui.snapshots.invalidate();
        }
    }
}

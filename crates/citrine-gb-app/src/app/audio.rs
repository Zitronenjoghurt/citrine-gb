use super::Citrine;
use egui::Context;

impl Citrine {
    pub fn try_start_audio(&mut self) {
        let Some(audio) = &self.audio else {
            return;
        };
        if audio.is_running() {
            return;
        }
        self.restart_audio();
    }

    pub(super) fn unlock_audio_on_interaction(&mut self, ctx: &Context) {
        let interacted = ctx.input(|i| {
            i.pointer.any_pressed()
                || i.events.iter().any(|e| {
                    matches!(
                        e,
                        egui::Event::Key { pressed: true, .. }
                            | egui::Event::Touch {
                                phase: egui::TouchPhase::Start,
                                ..
                            }
                    )
                })
        });
        if !interacted {
            return;
        }

        let stalled = std::mem::take(&mut self.audio_stalled);
        let Some(audio) = self.audio.as_mut() else {
            return;
        };

        audio.mark_user_activated();

        if audio.is_running() && !stalled {
            audio.resume();
            return;
        }

        self.restart_audio();
    }

    pub(super) fn restart_audio(&mut self) {
        let Some(audio) = &mut self.audio else {
            return;
        };

        let device = self.ui.settings.audio_device.clone();
        match audio.start(device.as_deref(), self.ui.settings.audio_queue_ms) {
            Ok(link) => {
                self.emulator.attach_audio(link);
                self.ui.settings.dirty = true;
            }
            Err(err) => {
                self.toasts.error(format!("Failed to start audio: {err}"));
            }
        }
    }
}

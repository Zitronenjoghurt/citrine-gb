use super::Citrine;
use super::{events, widgets};
use crate::app::tabs::Tab;
use crate::app::ui_state::hotkeys::HotkeyAction;
use crate::icons;
use crate::media::recording::{self, Availability, FinishedRecording, Outcome, StopReason};
use crate::media::{Frame as MediaFrame, MediaId, MediaItem, MediaKind, NewMedia};
use crate::progress::Progress;
use egui::Context;

const CODEC_LOAD: &str = "codec";
const RECORDING_LOAD: &str = "recording";

impl Citrine {
    pub(super) fn media_saved(&mut self, id: MediaId, touch: bool) {
        let kind = id.kind().unwrap_or(MediaKind::Screenshot);
        let in_play = touch || self.ui.settings.focus_mode || self.media_would_cover_game();
        let toast = self
            .toasts
            .success(format!("{} saved to Media", kind.label()));
        if in_play {
            toast.button("Open", events::AppEvent::ShowMedia { id });
        } else {
            self.show_media(id, touch);
        }
    }

    pub(super) fn media_added(&mut self, id: MediaId) {
        let kind = id.kind().unwrap_or(MediaKind::Screenshot);
        self.toasts
            .success(format!("{} saved to Media", kind.label()))
            .button("Open", events::AppEvent::ShowMedia { id });
    }

    pub(super) fn show_media(&mut self, id: MediaId, touch: bool) {
        if self.ui.settings.focus_mode && !touch {
            self.toggle_focus_mode();
        }
        self.ui.media.highlight(id);
        self.show_tab(Tab::Media, touch);
    }

    pub(super) fn take_screenshot(&mut self) {
        match crate::media::save_screenshot(
            &self.emulator,
            &mut self.media,
            self.ui.settings.screenshot_scale,
        ) {
            Ok(item) => self
                .events
                .push(events::AppEvent::MediaSaved { id: item.id }),
            Err(message) => {
                self.toasts.error(message);
            }
        };
    }

    pub(super) fn recording_button(
        &self,
        ui: &mut egui::Ui,
        size: Option<egui::Vec2>,
    ) -> egui::Response {
        let keys = self
            .ui
            .settings
            .hotkeys
            .keys_for(HotkeyAction::ToggleRecording);
        let hint = |text: &str| match keys.first() {
            Some(hotkey) => format!("{text} ({hotkey})"),
            None => text.to_string(),
        };
        let icon_size = size.map(|_| 18.0);
        let recorder = &self.emulator.screen_recorder;
        if recorder.is_recording() {
            let label = match size {
                Some(_) => format!("{}  {}", icons::RECORD, recorder.elapsed_label()),
                None => icons::STOP.to_string(),
            };
            let mut text = egui::RichText::new(label).color(ui.visuals().error_fg_color);
            if let Some(px) = icon_size {
                text = text.size(px);
            }
            let button = egui::Button::new(text).min_size(size.unwrap_or_default());
            return ui.add(button).on_hover_text(hint("Stop recording"));
        }
        let mut text = egui::RichText::new(icons::RECORD);
        if let Some(px) = icon_size {
            text = text.size(px);
        }
        let button = egui::Button::new(text).min_size(size.unwrap_or_default());
        let loaded = self.emulator.has_game();
        match recording::availability(
            self.recording_source(),
            self.emulator.sample_rate(),
            self.ui.settings.recording_scale,
        ) {
            Availability::Ready | Availability::Checking | Availability::Download => ui
                .add_enabled(loaded, button)
                .on_hover_text(hint("Record video")),
            Availability::Missing(reason) => {
                ui.add_enabled(false, button).on_disabled_hover_text(reason)
            }
        }
    }

    fn recording_source(&self) -> recording::Source {
        recording::Source::new(self.emulator.video(), self.emulator.timing())
    }

    pub(super) fn toggle_recording(&mut self) {
        if self.emulator.screen_recorder.is_recording() {
            self.emulator.screen_recorder.stop();
            return;
        }
        if !self.emulator.has_game() {
            self.toasts.error("Load a ROM before recording");
            return;
        }
        let rate = self.emulator.sample_rate();
        let scale = self.ui.settings.recording_scale;
        let source = self.recording_source();
        match recording::availability(source, rate, scale) {
            Availability::Ready => {}
            Availability::Checking => {
                self.record_when_ready = true;
                return;
            }
            Availability::Download => {
                self.codec_prompt = true;
                return;
            }
            Availability::Missing(reason) => {
                self.toasts.error(reason);
                return;
            }
        }
        let new = NewMedia::current(MediaKind::Recording, &self.emulator);
        if let Err(err) = self
            .emulator
            .screen_recorder
            .start(new, source, rate, scale)
        {
            self.toasts
                .error(format!("Could not start recording: {err}"));
        }
    }

    pub(super) fn codec_confirm(&mut self, ctx: &Context) {
        if !self.codec_prompt {
            return;
        }
        let choice = widgets::confirm::confirm_with_detail(
            ctx,
            "codec_confirm",
            "Recording needs Cisco's video codec, a one-time download of about 600 KB.",
            Some(recording::CODEC_NOTICE),
            "Download",
        );
        let Some(download) = choice else {
            return;
        };
        self.codec_prompt = false;
        if recording::download_codec(download) {
            self.record_when_ready = true;
        } else {
            self.toasts
                .info("Recording needs the video codec, so nothing was recorded");
        }
    }

    pub(super) fn finish_recordings(&mut self) {
        if let Some(err) = recording::take_codec_failure() {
            let pending = std::mem::take(&mut self.record_when_ready);
            self.toasts.error(if pending {
                format!("Could not download the video codec, so nothing was recorded: {err}")
            } else {
                format!("Could not download the video codec: {err}")
            });
        }
        if self.record_when_ready
            && !matches!(
                recording::availability(
                    self.recording_source(),
                    self.emulator.sample_rate(),
                    self.ui.settings.recording_scale,
                ),
                Availability::Checking
            )
        {
            self.record_when_ready = false;
            self.toggle_recording();
        }
        match self.emulator.screen_recorder.take_stopped() {
            Some(StopReason::SoundChanged) => {
                self.toasts
                    .info("Recording stopped because the sound output changed");
            }
            Some(StopReason::TooSlow) => {
                self.toasts
                    .error("Recording stopped because this device couldn't keep up");
            }
            Some(StopReason::TooLong) => {
                let minutes = recording::max_length().map_or(0, |max| max.as_secs() / 60);
                self.toasts.info(format!(
                    "Recording stopped at the browser's limit of {minutes} minutes"
                ));
            }
            Some(StopReason::Asked | StopReason::Failed) | None => {}
        }
        self.store_recordings();
    }

    pub(super) fn show_loads(&mut self) {
        let now = web_time::Instant::now();
        match recording::codec_progress().filter(|_| self.record_when_ready) {
            Some(progress) => {
                self.toasts
                    .load(CODEC_LOAD, "Downloading the video codec", progress, now);
            }
            None => self.toasts.load_done(CODEC_LOAD),
        }
        if self.emulator.screen_recorder.is_saving() {
            self.toasts.load(
                RECORDING_LOAD,
                "Saving the recording",
                Progress::unknown(),
                now,
            );
        } else {
            self.toasts.load_done(RECORDING_LOAD);
        }
    }

    pub(super) fn store_recordings(&mut self) {
        while let Some(outcome) = self.emulator.screen_recorder.poll() {
            match outcome {
                Outcome::Saved(done) => match self.store_recording(*done) {
                    Ok(item) => {
                        self.events
                            .push(events::AppEvent::MediaSaved { id: item.id });
                        continue;
                    }
                    Err(err) => self
                        .toasts
                        .error(format!("Could not save the recording: {err}")),
                },
                Outcome::Empty => self
                    .toasts
                    .info("Nothing was recorded while the game was paused"),
                Outcome::Failed(err) => self
                    .toasts
                    .error(format!("Could not save the recording: {err}")),
            };
        }
    }

    pub(super) fn store_recording(
        &mut self,
        done: FinishedRecording,
    ) -> std::io::Result<MediaItem> {
        let mut new = done.info;
        new.width = done.width as u32;
        new.height = done.height as u32;
        new.length_ms = Some(done.length.as_millis() as u64);
        if done.thumbnail.len() == new.frame.rgba.len() {
            new.frame = MediaFrame {
                rgba: done.thumbnail,
                ..new.frame
            };
        }
        #[cfg(not(target_arch = "wasm32"))]
        let item = self.media.add_recording(&done.output.path, new)?;
        #[cfg(target_arch = "wasm32")]
        let item = self.media.add_recording_blob(done.output.blob, new)?;
        Ok(item)
    }
}

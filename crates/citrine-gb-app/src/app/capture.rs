use super::Citrine;
use super::{events, widgets};
use crate::app::tabs::Tab;
use crate::app::ui_state::hotkeys::HotkeyAction;
use crate::app::widgets::record_bar::{RecordBarAction, RecordBarFacts, record_bar};
use crate::app::widgets::replay_bar::{ReplayBarFacts, replay_bar};
use crate::icons;
use crate::media::recording::{self, Availability, FinishedRecording, Outcome, StopReason};
use crate::media::render::RenderVideo;
use crate::media::{Frame as MediaFrame, MediaId, MediaItem, MediaKind, NewMedia};
use crate::progress::Progress;
use citrine_gb_replay::Replay;
use egui::Context;
use std::sync::mpsc;

pub(super) struct ParkedReplay {
    replay: Replay,
    new: Option<NewMedia>,
    frames: crate::frame_log::Recorded,
    begun: Option<crate::app::recording::Begun>,
}

#[cfg(not(target_arch = "wasm32"))]
type PackedFile = std::path::PathBuf;
#[cfg(target_arch = "wasm32")]
type PackedFile = Vec<u8>;
type Packed = Result<(PackedFile, recording::Output), (String, recording::Output)>;

pub(super) struct VideoReplays {
    packing: usize,
    tx: mpsc::Sender<(NewMedia, Packed)>,
    rx: mpsc::Receiver<(NewMedia, Packed)>,
}

impl Default for VideoReplays {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self { packing: 0, tx, rx }
    }
}

const CODEC_LOAD: &str = "codec";
const RECORDING_LOAD: &str = "recording";

impl Citrine {
    pub(super) fn media_saved(&mut self, id: MediaId, touch: bool) {
        let kind = id.kind().unwrap_or(MediaKind::Screenshot);
        let in_play = touch || self.ui.focus_mode || self.media_would_cover_game();
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
        if self.ui.focus_mode && !touch {
            self.toggle_focus_mode();
        }
        self.ui.media.highlight(id);
        self.show_tab(Tab::Media, touch);
    }

    pub(super) fn take_screenshot(&mut self) {
        match crate::media::save_screenshot(
            &mut self.emulator,
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
        if self.emulator.is_recording() {
            let button = match size {
                Some(size) => {
                    let label = format!("{}  {}", icons::RECORD, self.emulator.recording_label());
                    let text = egui::RichText::new(label)
                        .color(ui.visuals().error_fg_color)
                        .size(18.0);
                    egui::Button::new(text).min_size(size)
                }
                None => egui::Button::new(icons::RECORD).selected(true),
            };
            return ui.add(button).on_hover_text(hint("Stop recording"));
        }
        let mut text = egui::RichText::new(icons::RECORD);
        if let Some(px) = icon_size {
            text = text.size(px);
        }
        let button = egui::Button::new(text).min_size(size.unwrap_or_default());
        let loaded = self.emulator.has_game();
        if self.emulator.can_tape() {
            return ui.add_enabled(loaded, button).on_hover_text(hint("Record"));
        }
        match recording::availability(
            self.recording_source(),
            self.emulator.sample_rate(),
            self.ui.settings.recording_scale,
        ) {
            Availability::Ready | Availability::Checking | Availability::Download => {
                ui.add_enabled(loaded, button).on_hover_text(hint("Record"))
            }
            Availability::Missing(reason) => {
                ui.add_enabled(false, button).on_disabled_hover_text(reason)
            }
        }
    }

    fn recording_source(&self) -> recording::Source {
        recording::Source::laid_out(self.emulator.shown_layout(), self.emulator.timing())
    }

    pub(super) fn toggle_recording(&mut self) {
        if self.emulator.is_taping() {
            self.stop_tape_recording();
            return;
        }
        if self.emulator.screen_recorder.is_recording() {
            self.emulator.screen_recorder.stop();
            return;
        }
        if !self.emulator.has_game() {
            self.toasts.error("Load a ROM before recording");
            return;
        }
        if self.emulator.can_tape() {
            self.start_tape_recording();
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
        let mut new = NewMedia::current(MediaKind::Recording, &mut self.emulator);
        if let Some(header) = self.emulator.video_header() {
            new.kind = MediaKind::Replay;
            new.video = true;
            new.replay = Some(Box::new(Replay {
                header,
                ..Replay::default()
            }));
        }
        if let Err(err) =
            self.emulator
                .screen_recorder
                .start(&self.files.spawner, new, source, rate, scale)
        {
            self.toasts
                .error(format!("Could not start recording: {err}"));
            return;
        }
        let layout = self.emulator.shown_layout();
        self.emulator.screen_recorder.lay_out(layout);
        if self.ui.settings.record_frame_times {
            self.ui.frame_log.start_recording(web_time::Instant::now());
        }
    }

    fn start_tape_recording(&mut self) {
        if let Some(replay) = self.emulator.take_cut_recording() {
            self.keep_replay(replay);
        }
        let new = NewMedia::current(MediaKind::Replay, &mut self.emulator);
        if !self.emulator.start_tape_recording() {
            self.toasts.error("Could not start recording");
            return;
        }
        self.emulator.fast_forward = false;
        self.tape_media = Some(new);
        self.keep_tape_rom();
        if self.ui.settings.record_frame_times {
            self.ui.frame_log.start_recording(web_time::Instant::now());
        }
    }

    pub(super) fn stop_tape_recording(&mut self) {
        match self.emulator.stop_tape_recording() {
            Some(replay) => self.keep_replay(replay),
            None => {
                self.tape_media = None;
                self.ui.frame_log.stop_recording();
                self.toasts.error("Could not save the recording");
            }
        }
    }

    pub(super) fn keep_replay(&mut self, replay: Replay) {
        let new = self.tape_media.take();
        match self.ui.frame_log.stop_recording() {
            Some(frames) if frames.frames > 0 => self.parked_replays.push(ParkedReplay {
                replay,
                new,
                frames,
                begun: self.ui.record_watch.take_begun(),
            }),
            _ => self.save_replay(replay, new),
        }
    }

    pub(super) fn store_parked_replays(&mut self, ctx: Option<&Context>) {
        for parked in std::mem::take(&mut self.parked_replays) {
            let mut replay = parked.replay;
            if let Some(ctx) = ctx {
                replay.perf = Some(self.ui.record_watch.finish(
                    parked.frames,
                    parked.begun,
                    ctx,
                    &mut self.emulator,
                    &self.ui.settings,
                ));
            }
            self.save_replay(replay, parked.new);
        }
    }

    pub(super) fn record_bar_action(&mut self, action: RecordBarAction) {
        match action {
            RecordBarAction::StartOver => self.start_over(),
            RecordBarAction::Stop => self.toggle_recording(),
            RecordBarAction::FrameTimes(true) => {
                if self.emulator.is_recording() {
                    self.ui.frame_log.start_recording(web_time::Instant::now());
                }
            }
            RecordBarAction::FrameTimes(false) => {
                self.ui.frame_log.stop_recording();
            }
        }
    }

    pub(super) fn record_row(&mut self, ui: &mut egui::Ui) -> Option<f32> {
        if let Some(facts) = ReplayBarFacts::of(&self.emulator, true) {
            let shown = egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(12, 0))
                .show(ui, |ui| replay_bar(ui, &facts, self.emulator.tape_times()));
            if let Some(action) = shown.inner {
                self.replay_bar_action(action);
            }
            return Some(shown.response.rect.bottom());
        }
        let facts = RecordBarFacts::of(&self.emulator, &self.ui, true)?;
        let action = egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(12, 0))
            .show(ui, |ui| record_bar(ui, &facts))
            .inner;
        if let Some(action) = action {
            self.record_bar_action(action);
        }
        None
    }

    fn start_over(&mut self) {
        if !self.emulator.restart_tape_recording() {
            self.toasts.error("Could not go back to the start");
            return;
        }
        if self.ui.frame_log.is_recording() {
            self.ui.frame_log.start_recording(web_time::Instant::now());
        }
    }

    pub(super) fn save_replay(&mut self, replay: Replay, new: Option<NewMedia>) {
        let header = &replay.header;
        let frame_times = replay.perf.is_some();
        let mut new = new.unwrap_or_else(|| NewMedia {
            kind: MediaKind::Replay,
            game: header.game.clone(),
            rom_key: None,
            model: None,
            created_at: 0,
            width: 0,
            height: 0,
            length_ms: None,
            frame_times: false,
            video: false,
            replay: None,
            frame: MediaFrame {
                width: 0,
                height: 0,
                rgba: Vec::new(),
            },
        });
        new.length_ms = Some((header.frames as f64 * 1000.0 / header.frame_rate) as u64);
        new.created_at = crate::storage::now_unix();
        new.frame_times = frame_times;
        let saved = replay
            .write()
            .map_err(|err| err.to_string())
            .and_then(|bytes| {
                self.media
                    .add_file(new, &bytes)
                    .map_err(|err| err.to_string())
            });
        match saved {
            Ok(item) => {
                if self.ui.settings.render_video == RenderVideo::RightAway
                    && replay.video.is_none()
                    && replay.header.no_inputs.is_none()
                {
                    self.render_video(item.id.clone(), Box::new(replay));
                }
                self.video_renders.forget_tape_rom();
                self.events
                    .push(events::AppEvent::MediaSaved { id: item.id });
            }
            Err(err) => {
                self.toasts
                    .error(format!("Could not save the recording: {err}"));
            }
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
        if recording::download_codec(&self.files.spawner, download) {
            self.record_when_ready = true;
        } else {
            self.toasts
                .info("Recording needs the video codec, so nothing was recorded");
        }
    }

    pub(super) fn finish_recordings(&mut self, ctx: &Context) {
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
        if let Some(replay) = self.emulator.take_cut_recording() {
            self.keep_replay(replay);
        }
        if !self.emulator.is_recording()
            && let Some(frames) = self.ui.frame_log.stop_recording()
            && frames.frames > 0
        {
            let begun = self.ui.record_watch.take_begun();
            let perf = self.ui.record_watch.finish(
                frames,
                begun,
                ctx,
                &mut self.emulator,
                &self.ui.settings,
            );
            if let Some(replay) = self
                .emulator
                .screen_recorder
                .last_stopped()
                .and_then(|info| info.replay.as_mut())
                && replay.perf.is_none()
            {
                replay.perf = Some(perf);
            }
        }
        self.store_parked_replays(Some(ctx));
        self.store_recordings();
        self.store_video_replays();
        self.tick_video_renders(ctx);
        self.tick_replay_show();
        self.tick_clip(ctx);
        if self.video_replays.packing > 0 {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
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
        if self.emulator.screen_recorder.is_saving() || self.video_replays.packing > 0 {
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
                Outcome::Saved(done) => self.store_recording(*done, false),
                Outcome::Empty => {
                    self.toasts
                        .info("Nothing was recorded while the game was paused");
                }
                Outcome::Failed(err) => {
                    self.toasts
                        .error(format!("Could not save the recording: {err}"));
                }
            };
        }
    }

    pub(super) fn store_recording(&mut self, done: FinishedRecording, wait: bool) {
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
        let Some(mut replay) = new.replay.take() else {
            let saved = self.add_recording(done.output, new);
            self.media_stored(saved.map_err(|err| err.to_string()));
            return;
        };
        replay.header.frames = done.frames;
        replay.thumb = crate::utils::screenshot::encode_png(
            new.frame.width as usize,
            new.frame.height as usize,
            &new.frame.rgba,
        );
        new.frame_times = replay.perf.is_some();
        self.video_replays.packing += 1;
        let tx = self.video_replays.tx.clone();
        let output = done.output;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let pack = move || {
                let _ = tx.send((new, output.pack(*replay)));
            };
            if wait {
                pack();
            } else {
                self.files.spawner.run("recording", pack);
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = wait;
            citrine_gb_threads::spawn_local(async move {
                let _ = tx.send((new, output.pack(*replay).await));
            });
        }
    }

    pub(super) fn store_video_replays(&mut self) {
        while let Ok((new, packed)) = self.video_replays.rx.try_recv() {
            self.video_replays.packing = self.video_replays.packing.saturating_sub(1);
            let saved = match packed {
                Ok((file, output)) => match self.add_packed(file, new.clone()) {
                    Ok(item) => {
                        recording::discard(output);
                        Ok(item)
                    }
                    Err(err) => {
                        log::warn!("could not keep the packed recording, keeping its video: {err}");
                        self.keep_video(output, new)
                    }
                },
                Err((err, output)) => {
                    log::warn!("could not pack the recording, keeping its video: {err}");
                    self.keep_video(output, new)
                }
            };
            self.media_stored(saved);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn add_packed(&mut self, file: PackedFile, new: NewMedia) -> std::io::Result<MediaItem> {
        let added = self.media.add_recording(&file, new);
        if added.is_err() {
            let _ = std::fs::remove_file(&file);
        }
        added
    }

    #[cfg(target_arch = "wasm32")]
    fn add_packed(&mut self, file: PackedFile, new: NewMedia) -> std::io::Result<MediaItem> {
        self.media.add_file(new, &file)
    }

    fn keep_video(
        &mut self,
        output: recording::Output,
        new: NewMedia,
    ) -> Result<MediaItem, String> {
        let new = NewMedia {
            kind: MediaKind::Recording,
            video: false,
            frame_times: false,
            ..new
        };
        self.add_recording(output, new)
            .map_err(|err| err.to_string())
    }

    fn media_stored(&mut self, saved: Result<MediaItem, String>) {
        match saved {
            Ok(item) => self
                .events
                .push(events::AppEvent::MediaSaved { id: item.id }),
            Err(err) => {
                self.toasts
                    .error(format!("Could not save the recording: {err}"));
            }
        }
    }

    fn add_recording(
        &mut self,
        output: recording::Output,
        new: NewMedia,
    ) -> std::io::Result<MediaItem> {
        #[cfg(not(target_arch = "wasm32"))]
        let item = self.media.add_recording(&output.path, new)?;
        #[cfg(target_arch = "wasm32")]
        let item = self.media.add_recording_blob(output.blob, new)?;
        Ok(item)
    }
}

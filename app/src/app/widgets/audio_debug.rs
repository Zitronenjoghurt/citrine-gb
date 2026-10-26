use crate::app::widgets::settings::form::Form;
use crate::audio::Audio;
use crate::emulator::Emulator;
use citrine_gb::utils::ema::EMA;
use egui::{Label, Response, Ui, Widget};

const SMOOTHING_PERIODS: usize = 30;

#[derive(Default)]
pub struct AudioDebugState {
    queued_frames: EMA<f32>,
    rate_adjust: EMA<f64>,
}

impl AudioDebugState {
    fn update(&mut self, emulator: &Emulator) {
        let Some(feed) = emulator.audio_feed() else {
            return;
        };
        self.queued_frames
            .update(feed.queued_frames() as f32, SMOOTHING_PERIODS);
        self.rate_adjust
            .update(1.0 + feed.rate_adjust(), SMOOTHING_PERIODS);
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

pub struct AudioDebug<'a> {
    audio: &'a mut Audio,
    emulator: &'a mut Emulator,
    state: &'a mut AudioDebugState,
}

impl<'a> AudioDebug<'a> {
    pub fn new(
        audio: &'a mut Audio,
        emulator: &'a mut Emulator,
        state: &'a mut AudioDebugState,
    ) -> Self {
        Self {
            audio,
            emulator,
            state,
        }
    }
}

impl Widget for AudioDebug<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        self.state.update(self.emulator);

        let rate = self.audio.get_sample_rate().max(1) as f32;
        let ms = |frames: f32| frames * 1000.0 / rate;

        let callback = self.audio.callback_frames();
        let queued = self.state.queued_frames.value();
        let target = self
            .emulator
            .audio_feed()
            .map(|feed| feed.target_frames())
            .unwrap_or(0);

        let value = |ui: &mut Ui, text: String| ui.add(Label::new(text).truncate());
        let stats = self.audio.stats();
        let (min, max) = self.audio.get_supported_buffer_size();
        let ratio = self.state.rate_adjust.value().max(f64::MIN_POSITIVE);
        let resampling = rate as f64 * ratio;
        let drift = (ratio - 1.0) * 100.0;

        let response = ui
            .scope(|ui| {
                Form::show(ui, "audio_debug", |form| {
                    form.striped();
                    form.row("Sample rate", |ui| {
                        value(ui, format!("{} Hz", self.audio.get_sample_rate()))
                    });
                    form.row("Channel count", |ui| {
                        value(ui, self.audio.get_channel_count().to_string())
                    });
                    form.row("Supported buffer size", |ui| {
                        value(ui, format!("{min}/{max}"))
                    });
                    form.row("Callback size", |ui| {
                        value(
                            ui,
                            format!("{callback} frames ({:.0} ms)", ms(callback as f32)),
                        )
                        .on_hover_text("How much sound the device takes from the queue at a time")
                    });
                    form.row("Queued / target", |ui| {
                        value(
                            ui,
                            format!(
                                "{queued:.0} / {target} frames ({:.0} / {:.0} ms)",
                                ms(queued),
                                ms(target as f32)
                            ),
                        )
                        .on_hover_text(
                            "Queued audio is latency you can hear; it should sit near target",
                        )
                    });
                    form.row("Resampling at", |ui| {
                        value(ui, format!("{resampling:.0} Hz ({drift:+.2}%)")).on_hover_text(
                            "Set slightly off the device rate to hold the queue at target",
                        )
                    });

                    if let Some(desc) = self.audio.get_device_description() {
                        form.row("Device name", |ui| value(ui, desc.name().to_string()));
                        form.row("Device type", |ui| value(ui, desc.device_type().to_string()));
                        form.row("Interface type", |ui| {
                            value(ui, desc.interface_type().to_string())
                        });
                        form.row("Device manufacturer", |ui| {
                            value(ui, desc.manufacturer().unwrap_or("Unknown").to_string())
                        });
                        form.row("Device driver", |ui| {
                            value(ui, desc.driver().unwrap_or("Unknown").to_string())
                        });
                    }

                    form.row("Underruns", |ui| {
                        value(
                            ui,
                            format!(
                                "{} times, {:.0} ms silent",
                                stats.underruns(),
                                ms(stats.underrun_frames() as f32)
                            ),
                        )
                        .on_hover_text("The queue ran dry, so the sound faded out until it refilled")
                    });
                    form.row("Overruns", |ui| {
                        value(
                            ui,
                            format!(
                                "{} times, {:.0} ms skipped",
                                stats.overruns(),
                                ms(stats.overrun_frames() as f32)
                            ),
                        )
                        .on_hover_text(
                            "The queue ran far past target, so sound was skipped with a short crossfade",
                        )
                    });
                    form.row("Sample min/max", |ui| {
                        value(
                            ui,
                            format!("{:.2}/{:.2}", stats.min_sample(), stats.max_sample()),
                        )
                        .on_hover_text("Peak hold since the last reset")
                    });
                });
            })
            .response;

        if ui.button("Reset stats").clicked() {
            self.audio.reset_stats();
            self.state.reset();
        }

        response
    }
}

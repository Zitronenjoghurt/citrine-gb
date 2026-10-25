use crate::audio::Audio;
use crate::emulator::Emulator;
use citrine_gb::utils::ema::EMA;
use egui::{Grid, Response, Ui, Widget};

const SMOOTHING_PERIODS: usize = 30;

const MAX_SAMPLE_GAP_SECS: f32 = 1.0;

struct Counters {
    at: web_time::Instant,
    underrun: u64,
    overrun: u128,
}

#[derive(Default)]
pub struct AudioDebugState {
    queued_frames: EMA<f32>,
    resample_rate: EMA<f32>,
    underrun_per_sec: EMA<f32>,
    overrun_per_sec: EMA<f32>,
    last: Option<Counters>,
}

impl AudioDebugState {
    fn update(&mut self, audio: &Audio, emulator: &Emulator) {
        self.queued_frames
            .update(emulator.audio_queued_frames() as f32, SMOOTHING_PERIODS);
        self.resample_rate
            .update(emulator.gb.apu.output_sample_rate as f32, SMOOTHING_PERIODS);

        let at = web_time::Instant::now();
        let underrun = audio.get_underrun_samples();
        let overrun = emulator.audio_overrun_samples;

        if let Some(prev) = self.last.replace(Counters {
            at,
            underrun,
            overrun,
        }) {
            let secs = at.duration_since(prev.at).as_secs_f32();
            if secs <= 0.0 || secs > MAX_SAMPLE_GAP_SECS {
                return;
            }

            let underruns = underrun.saturating_sub(prev.underrun) as f32;
            let overruns = overrun.saturating_sub(prev.overrun) as f32;
            self.underrun_per_sec
                .update(underruns / secs, SMOOTHING_PERIODS);
            self.overrun_per_sec
                .update(overruns / secs, SMOOTHING_PERIODS);
        }
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
        self.state.update(self.audio, self.emulator);

        let rate = self.audio.get_sample_rate().max(1) as f32;
        let ms = |frames: f32| frames * 1000.0 / rate;

        let callback = self.audio.callback_frames();
        let queued = self.state.queued_frames.value();
        let target = self.emulator.audio_target_frames();

        #[cfg(target_arch = "wasm32")]
        let scheduled_frames = Some(callback * 2);
        #[cfg(not(target_arch = "wasm32"))]
        let scheduled_frames: Option<usize> = None;

        let response = Grid::new("audio_debug_grid")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Sample Rate");
                ui.label(format!("{} Hz", self.audio.get_sample_rate()));
                ui.end_row();

                ui.label("Channel Count");
                ui.label(self.audio.get_channel_count().to_string());
                ui.end_row();

                let (min, max) = self.audio.get_supported_buffer_size();
                ui.label("Supported Buffer Size");
                ui.label(format!("{}/{}", min, max));
                ui.end_row();

                ui.label("Callback size");
                ui.label(format!("{callback} frames ({:.0} ms)", ms(callback as f32)))
                    .on_hover_text(
                        "On the web this is how long the main thread has to refill the \
                     device before a gap is audible",
                    );
                ui.end_row();

                if let Some(scheduled) = scheduled_frames {
                    ui.label("Scheduled ahead");
                    ui.label(format!(
                        "{scheduled} frames ({:.0} ms)",
                        ms(scheduled as f32)
                    ))
                    .on_hover_text(
                        "Already handed to the audio context and waiting to play. \
                         This is latency on top of the queue, not part of it",
                    );
                    ui.end_row();
                }

                ui.label("Queued / target");
                ui.label(format!(
                    "{queued:.0} / {target} frames ({:.0} / {:.0} ms)",
                    ms(queued),
                    ms(target as f32)
                ))
                .on_hover_text("Queued audio is latency you can hear; it should sit near target");
                ui.end_row();

                if let Some(scheduled) = scheduled_frames {
                    ui.label("Output latency");
                    ui.label(format!("~{:.0} ms", ms(queued + scheduled as f32)))
                        .on_hover_text("The queue plus what is already scheduled");
                    ui.end_row();
                }

                let resampling = self.state.resample_rate.value();
                let drift = (resampling / rate - 1.0) * 100.0;
                ui.label("Resampling at");
                ui.label(format!("{resampling:.0} Hz ({drift:+.2}%)"))
                    .on_hover_text("Set slightly off the device rate to hold the queue at target");
                ui.end_row();

                if let Some(desc) = self.audio.get_device_description() {
                    ui.label("Device name");
                    ui.label(desc.name());
                    ui.end_row();

                    ui.label("Device type");
                    ui.label(desc.device_type().to_string());
                    ui.end_row();

                    ui.label("Interface type");
                    ui.label(desc.interface_type().to_string());
                    ui.end_row();

                    ui.label("Device manufacturer");
                    ui.label(desc.manufacturer().unwrap_or("Unknown"));
                    ui.end_row();

                    ui.label("Device driver");
                    ui.label(desc.driver().unwrap_or("Unknown"));
                    ui.end_row();
                }

                ui.label("Underruns");
                ui.label(format!(
                    "{} total, {:.0}/s",
                    self.audio.get_underrun_samples(),
                    self.state.underrun_per_sec.value()
                ))
                .on_hover_text("Samples the device asked for that were not ready");
                ui.end_row();

                ui.label("Overruns");
                ui.label(format!(
                    "{} total, {:.0}/s",
                    self.emulator.audio_overrun_samples,
                    self.state.overrun_per_sec.value()
                ))
                .on_hover_text("Samples dropped because the queue was already full");
                ui.end_row();

                ui.label("Sample min/max");
                ui.label(format!(
                    "{:.2}/{:.2}",
                    self.audio.get_min_sample(),
                    self.audio.get_max_sample()
                ))
                .on_hover_text("Peak hold since the last reset");
                ui.end_row();
            })
            .response;

        if ui.button("Reset stats").clicked() {
            self.audio.reset_stats();
            self.emulator.audio_overrun_samples = 0;
            self.state.reset();
        }

        response
    }
}

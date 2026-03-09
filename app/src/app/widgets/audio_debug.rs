use crate::audio::Audio;
use crate::emulator::Emulator;
use egui::{Grid, Response, Ui, Widget};

pub struct AudioDebug<'a> {
    audio: &'a mut Audio,
    emulator: &'a mut Emulator,
}

impl<'a> AudioDebug<'a> {
    pub fn new(audio: &'a mut Audio, emulator: &'a mut Emulator) -> Self {
        Self { audio, emulator }
    }
}

impl Widget for AudioDebug<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        Grid::new("audio_debug_grid")
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

                let underrun = self.audio.get_underrun_samples();
                let overrun = self.emulator.audio_overrun_samples;
                let frames = self.emulator.total_frames;

                ui.label("Total underrun samples");
                ui.label(underrun.to_string());
                ui.end_row();

                ui.label("Total overrun samples");
                ui.label(overrun.to_string());
                ui.end_row();

                ui.label("Underrun per frame");
                ui.label(format!("{:.2}", underrun as f64 / frames as f64));
                ui.end_row();

                ui.label("Overrun per frame");
                ui.label(format!("{:.2}", overrun as f64 / frames as f64));
                ui.end_row();

                ui.label("Sample min/max");
                ui.label(format!(
                    "{:.2}/{:.2}",
                    self.audio.get_min_sample(),
                    self.audio.get_max_sample()
                ));
                ui.end_row();
            })
            .response
    }
}

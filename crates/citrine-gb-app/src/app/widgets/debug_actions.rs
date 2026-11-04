use crate::app::widgets::section::{section, section_with_hint};
use crate::emulator::Emulator;
use crate::icons;
use crate::utils::file_channels::FileChannels;
use crate::utils::file_saver::FileSaver;
use egui::{RichText, Widget};

pub struct DebugActions<'a> {
    emulator: &'a mut Emulator,
    files: &'a mut FileChannels,
}

impl<'a> DebugActions<'a> {
    pub fn new(emulator: &'a mut Emulator, files: &'a mut FileChannels) -> Self {
        Self { emulator, files }
    }
}

const RECORDING_HINT: &str = "Saves each button press with its M-cycle, so the lab can replay it. \
                              Load the ROM fresh first, or the replay drifts.";

impl Widget for DebugActions<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let mut response = None;
        if let Some(gb) = self.emulator.as_gb_mut() {
            section(ui, icons::FILE_CODE, "Console state", |ui| {
                let export = ui.button("Export as JSON");
                if export.clicked()
                    && let Ok(json) = gb.dump_json(crate::storage::now_unix())
                {
                    FileSaver::new("citrine_dump.json")
                        .add_filter("JSON", &["json"])
                        .dispatch(json.into_bytes(), self.files.save_tx.clone());
                }
                response = Some(export);
            });
        }
        section_with_hint(ui, icons::RECORD, "Input recording", RECORDING_HINT, |ui| {
            recording(ui, self.emulator, self.files);
        });
        response.unwrap_or_else(|| ui.response())
    }
}

fn recording(ui: &mut egui::Ui, emulator: &mut Emulator, files: &mut FileChannels) {
    let events = presses(emulator.recorder.event_count());
    if emulator.recorder.is_active() {
        let red = ui.visuals().error_fg_color;
        ui.label(RichText::new(format!("{} Recording, {events}", icons::RECORD)).color(red));
        if ui.button("Stop recording").clicked() {
            emulator.recorder.stop();
        }
        return;
    }
    if ui.button("Start recording").clicked() {
        emulator.start_recording();
    }
    if emulator.recorder.has_data() {
        ui.label(format!("{events} recorded"));
        if ui.button("Export recording").clicked()
            && let Some(json) = emulator.recorder.export_json()
        {
            FileSaver::new("citrine_recording.json")
                .add_filter("JSON", &["json"])
                .dispatch(json.into_bytes(), files.save_tx.clone());
        }
    }
}

fn presses(count: usize) -> String {
    if count == 1 {
        "1 press".to_owned()
    } else {
        format!("{count} presses")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_press_is_singular() {
        assert_eq!(presses(1), "1 press");
        assert_eq!(presses(0), "0 presses");
        assert_eq!(presses(12), "12 presses");
    }
}

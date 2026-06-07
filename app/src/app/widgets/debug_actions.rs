use crate::app::file_picker::FilePicker;
use egui::Widget;

pub struct DebugActions<'a> {
    emulator: &'a mut crate::emulator::Emulator,
    file_picker: &'a mut FilePicker,
}

impl<'a> DebugActions<'a> {
    pub fn new(
        emulator: &'a mut crate::emulator::Emulator,
        file_picker: &'a mut FilePicker,
    ) -> Self {
        Self {
            emulator,
            file_picker,
        }
    }
}

impl Widget for DebugActions<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let response = ui.button("Dump JSON");

        if response.clicked()
            && let Ok(json) = self.emulator.gb.dump_json()
        {
            self.file_picker.save("citrine_dump.json", json.as_bytes());
        }

        ui.separator();
        ui.label("Input Recording");
        ui.label(
            "Records button input with cycle timestamps for replay in the lab. \
             Start from a fresh ROM load for deterministic replay.",
        );

        if self.emulator.recorder.is_active() {
            ui.colored_label(
                egui::Color32::from_rgb(0xE0, 0x40, 0x40),
                format!(
                    "● Recording — {} event(s)",
                    self.emulator.recorder.event_count()
                ),
            );
            if ui.button("Stop Recording").clicked() {
                self.emulator.recorder.stop();
            }
        } else {
            if ui.button("Start Recording").clicked() {
                self.emulator.start_recording();
            }
            if self.emulator.recorder.has_data() {
                ui.label(format!(
                    "{} event(s) captured",
                    self.emulator.recorder.event_count()
                ));
                if ui.button("Export Recording").clicked()
                    && let Some(json) = self.emulator.recorder.export_json()
                {
                    self.file_picker
                        .save("citrine_recording.json", json.as_bytes());
                }
            }
        }

        response
    }
}

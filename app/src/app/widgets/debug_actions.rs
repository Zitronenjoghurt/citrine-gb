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

        response
    }
}

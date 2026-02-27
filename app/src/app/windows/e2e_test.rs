use crate::app::file_picker::FileIntent;
use crate::app::windows::{ActiveWindows, AppWindow};
use crate::Citrine;
use egui::{TextEdit, Ui, WidgetText};

pub struct E2ETestWindow;

impl E2ETestWindow {
    pub fn new() -> Self {
        Self
    }
}

impl AppWindow for E2ETestWindow {
    const ID: ActiveWindows = ActiveWindows::E2E_TEST;

    fn title(_app: &mut Citrine) -> impl Into<WidgetText> {
        "E2E Tests"
    }

    fn ui(&mut self, ui: &mut Ui, app: &mut Citrine) {
        #[cfg(target_arch = "wasm32")]
        {
            ui.small("This feature is not available in web.");
            return;
        }

        ui.vertical(|ui| {
            TextEdit::singleline(&mut app.ui.e2e.title)
                .hint_text("Title")
                .show(ui);

            TextEdit::multiline(&mut app.ui.e2e.description)
                .hint_text("Description")
                .show(ui);

            ui.separator();

            if ui.button("Create & Export").clicked() {
                app.file_picker.open(FileIntent::ExportE2E);
            }
        });
    }
}

use crate::app::tabs::TabViewer;
use egui::TextEdit;

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    #[cfg(target_arch = "wasm32")]
    {
        ui.small("This feature is not available in web.");
        return;
    }

    ui.vertical(|ui| {
        TextEdit::singleline(&mut viewer.ui.e2e.title)
            .hint_text("Title")
            .show(ui);

        TextEdit::multiline(&mut viewer.ui.e2e.description)
            .hint_text("Description")
            .show(ui);

        ui.separator();

        #[cfg(not(target_arch = "wasm32"))]
        if ui.button("Create & Export").clicked() {
            crate::utils::file_loader::FolderPicker::new()
                .title("Export E2E test to folder")
                .dispatch(viewer.files.folder_tx.clone());
        }
    });
}

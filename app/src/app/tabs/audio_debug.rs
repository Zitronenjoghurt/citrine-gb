use crate::app::tabs::TabViewer;
use crate::app::widgets::audio_debug::AudioDebug;
use egui::Widget;

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            if let Some(audio) = viewer.audio {
                AudioDebug::new(audio, viewer.emulator).ui(ui);
            } else {
                ui.small("Audio was not initialized.");
            }
        });
}

use crate::app::tabs::TabViewer;
use crate::app::widgets::info::{Controls, InfoPanel};
use egui::Widget;

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    let settings = &viewer.ui.settings;
    let controls = if crate::app::mobile_layout(settings.layout, ui.ctx()) {
        Controls::Touch
    } else {
        Controls::Desktop {
            bindings: &settings.bindings,
            hotkeys: &settings.hotkeys,
        }
    };
    InfoPanel::new(&mut viewer.ui.info, viewer.commonmark, controls).ui(ui);
}

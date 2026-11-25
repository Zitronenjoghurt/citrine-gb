use crate::app::tabs::TabViewer;
use crate::app::widgets::info::{Controls, InfoPanel};
use egui::Widget;

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    crate::app::widgets::update::card(ui, &mut viewer.ui.update, &viewer.files.spawner);
    let settings = &viewer.ui.settings;
    let touch = settings.layout.is_touch(ui.ctx());
    let controls = if touch {
        Controls::Touch
    } else {
        Controls::Desktop {
            bindings: &settings.bindings,
            hotkeys: &settings.hotkeys,
            quick_load: settings.quick_load,
        }
    };
    let panel = InfoPanel::new(&mut viewer.ui.info, viewer.commonmark, controls);
    if touch {
        panel.without_tabs().ui(ui);
    } else {
        panel.ui(ui);
    }
}

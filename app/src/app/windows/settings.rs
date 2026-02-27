use crate::app::widgets::settings::SettingsWidget;
use crate::app::windows::{ActiveWindows, AppWindow};
use crate::Citrine;
use egui::{Ui, Widget, WidgetText};

pub struct SettingsWindow;

impl SettingsWindow {
    pub fn new() -> Self {
        Self
    }
}

impl AppWindow for SettingsWindow {
    const ID: ActiveWindows = ActiveWindows::SETTINGS;

    fn title(_app: &mut Citrine) -> impl Into<WidgetText> {
        "Settings"
    }

    fn ui(&mut self, ui: &mut Ui, app: &mut Citrine) {
        SettingsWidget::new(&mut app.ui.settings).ui(ui);
    }
}

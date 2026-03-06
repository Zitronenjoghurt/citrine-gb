use crate::app::widgets::homebrew::HomebrewList;
use crate::app::windows::{ActiveWindows, AppWindow};
use crate::Citrine;
use egui::{Ui, Widget, WidgetText};

pub struct HomebrewWindow;

impl HomebrewWindow {
    pub fn new() -> Self {
        Self
    }
}

impl AppWindow for HomebrewWindow {
    const ID: ActiveWindows = ActiveWindows::HOMEBREW;

    fn title(_app: &mut Citrine) -> impl Into<WidgetText> {
        "Included Games"
    }

    fn ui(&mut self, ui: &mut Ui, app: &mut Citrine) {
        HomebrewList::new(app).ui(ui);
    }
}

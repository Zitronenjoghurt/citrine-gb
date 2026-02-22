use crate::app::widgets::time_control::TimeControl;
use crate::app::windows::{ActiveWindows, AppWindow};
use crate::Citrine;
use egui::{Ui, Widget, WidgetText};

#[derive(Default)]
pub struct TimeControlWindow;

impl TimeControlWindow {
    pub fn new() -> Self {
        Self
    }
}

impl AppWindow for TimeControlWindow {
    const ID: ActiveWindows = ActiveWindows::TIME_CONTROL;

    fn title(_app: &mut Citrine) -> impl Into<WidgetText> {
        "Time Control"
    }

    fn ui(&mut self, ui: &mut Ui, app: &mut Citrine) {
        TimeControl::new(&mut app.emulator).ui(ui);
    }
}

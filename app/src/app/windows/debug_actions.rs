use crate::app::windows::{ActiveWindows, AppWindow};
use crate::Citrine;
use egui::{Ui, WidgetText};

#[derive(Default)]
pub struct DebugActionsWindow;

impl DebugActionsWindow {
    pub fn new() -> Self {
        Self
    }
}

impl AppWindow for DebugActionsWindow {
    const ID: ActiveWindows = ActiveWindows::DEBUG_ACTIONS;

    fn title(_app: &mut Citrine) -> impl Into<WidgetText> {
        "Debug Actions"
    }

    fn ui(&mut self, ui: &mut Ui, app: &mut Citrine) {
        if ui.button("Clear Frame Buffer").clicked() {
            app.emulator.clear_frame(ui.ctx());
        }
    }
}

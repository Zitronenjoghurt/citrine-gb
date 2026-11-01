use crate::app::events::AppEventQueue;
use crate::app::tabs::Tab;
use crate::app::ui_state::settings::Settings;
use crate::icons;
use egui::{Response, Ui, Widget};

pub struct ApuWidget<'a> {
    settings: &'a mut Settings,
    events: &'a mut AppEventQueue,
}

impl<'a> ApuWidget<'a> {
    pub fn new(settings: &'a mut Settings, events: &'a mut AppEventQueue) -> Self {
        Self { settings, events }
    }
}

impl Widget for ApuWidget<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                if ui.button(icons::WAVE_SAWTOOTH).clicked() {
                    self.events.open_tab(Tab::ApuWaves);
                }

                for (index, enabled) in self.settings.channels.iter_mut().enumerate() {
                    self.settings.dirty |= ui.checkbox(enabled, format!("{}", index + 1)).changed();
                }
            });
        })
        .response
    }
}

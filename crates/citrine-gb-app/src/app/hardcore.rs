use crate::app::ui_state::settings::Settings;
use crate::icons;

pub(crate) const OFF: &str = "Off in hardcore";
pub(crate) const TURN_OFF: &str = "Turn hardcore off";

pub(crate) fn turn_off(settings: &mut Settings) {
    settings.achievements.hardcore = false;
    settings.dirty = true;
}

pub(crate) fn locked(label: &str) -> String {
    format!("{} {label}", icons::LOCK)
}

pub(crate) fn load_button(ui: &mut egui::Ui, hardcore: bool) -> egui::Response {
    if !hardcore {
        return ui.button("Load");
    }
    ui.add_enabled(false, egui::Button::new(locked("Load")))
        .on_disabled_hover_text(OFF)
}

pub(crate) fn off_note(ui: &mut egui::Ui, text: &str) -> bool {
    ui.add_space(16.0);
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new(text).weak());
        ui.add_space(8.0);
        ui.button(TURN_OFF).clicked()
    })
    .inner
}

impl super::Citrine {
    pub(super) fn hardcore_refused(&mut self, text: &str) {
        self.toasts
            .info(text)
            .button(TURN_OFF, super::events::AppEvent::HardcoreOff);
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn pause_line(frames: u32) -> String {
    let seconds = frames.div_ceil(60).max(1);
    format!("Hardcore limits pauses. Pausing in {seconds} s.")
}

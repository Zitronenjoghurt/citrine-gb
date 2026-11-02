use crate::icons;
use egui::Ui;
use std::time::Duration;
use web_time::Instant;

const COPIED_FOR: Duration = Duration::from_secs(2);

pub fn copy_button(ui: &mut Ui, text: &str, copied_at: Option<Instant>, now: Instant) -> bool {
    let copied = copied_at.is_some_and(|at| now.duration_since(at) < COPIED_FOR);
    let label = if copied {
        format!("{} Copied", icons::CHECK)
    } else {
        format!("{} Copy", icons::COPY)
    };
    let clicked = ui.button(label).clicked();
    if clicked {
        ui.ctx().copy_text(text.to_owned());
    }
    clicked
}

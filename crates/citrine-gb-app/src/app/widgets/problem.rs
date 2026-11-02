use crate::icons;
use egui::{RichText, Ui};

pub fn problem_text(ui: &Ui, text: &str) -> RichText {
    RichText::new(format!("{} {text}", icons::WARNING)).color(ui.visuals().warn_fg_color)
}

pub fn problem_line(ui: &mut Ui, text: &str) {
    ui.label(problem_text(ui, text));
}

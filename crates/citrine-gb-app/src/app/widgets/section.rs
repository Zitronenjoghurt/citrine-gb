use crate::app::widgets::info_mark::info_mark;
use egui::{RichText, Ui};

pub fn section(ui: &mut Ui, icon: &str, title: &str, body: impl FnOnce(&mut Ui)) {
    section_with(ui, icon, title, None, body);
}

pub fn section_with_hint(
    ui: &mut Ui,
    icon: &str,
    title: &str,
    hint: &str,
    body: impl FnOnce(&mut Ui),
) {
    section_with(ui, icon, title, Some(hint), body);
}

pub fn heading(ui: &mut Ui, icon: &str, title: &str) {
    ui.label(RichText::new(icon).size(15.0));
    ui.label(RichText::new(title).strong().size(15.0));
}

fn section_with(
    ui: &mut Ui,
    icon: &str,
    title: &str,
    hint: Option<&str>,
    body: impl FnOnce(&mut Ui),
) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        heading(ui, icon, title);
        if let Some(hint) = hint {
            info_mark(ui, hint);
        }
    });
    ui.separator();
    ui.add_space(2.0);
    body(ui);
    ui.add_space(14.0);
}

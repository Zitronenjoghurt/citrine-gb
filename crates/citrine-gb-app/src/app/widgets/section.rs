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
    if crate::app::mobile::touch_square(ui) > 0.0 {
        phone_section(ui, title, hint, body);
        return;
    }
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

fn phone_section(ui: &mut Ui, title: &str, hint: Option<&str>, body: impl FnOnce(&mut Ui)) {
    use crate::app::mobile::kit;
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        ui.label(
            RichText::new(title)
                .size(13.0)
                .color(ui.visuals().weak_text_color()),
        );
        if let Some(hint) = hint {
            info_mark(ui, hint);
        }
    });
    ui.add_space(2.0);
    egui::Frame::new()
        .fill(kit::card_fill(ui.visuals()))
        .corner_radius(egui::CornerRadius::same(kit::RADIUS))
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            body(ui);
        });
    ui.add_space(6.0);
}

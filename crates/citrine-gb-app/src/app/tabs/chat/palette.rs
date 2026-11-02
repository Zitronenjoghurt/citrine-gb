use super::bubble::{measure, row_with_stamp, stamp, stamp_width};
use super::clock::Clock;
use super::{Action, PALETTE_WIDTH, TEXT_STAMP_GAP};
use crate::app::ui_state::palette::{CustomPalette, PaletteLibrary};
use crate::app::widgets::palette_strip::palette_strip;
use crate::app::widgets::text::faint;
use crate::icons;
use crate::online::chat::ChatEntry;
use egui::{Button, Label, RichText, TextWrapMode, Ui};

pub(super) fn palette_card(
    ui: &mut Ui,
    entry: &ChatEntry,
    clock: &Clock,
    json: &str,
    palettes: &PaletteLibrary,
    actions: &mut Vec<Action>,
) {
    let Some(palette) = CustomPalette::from_shared(json) else {
        row_with_stamp(ui, entry, clock, |ui| {
            ui.label(faint("This palette can't be read"));
        });
        return;
    };
    ui.set_width(ui.available_width().min(card_width(ui)));
    palette_strip(ui, &[palette.rgba()]);
    ui.add_space(4.0);
    let name = RichText::new(format!("{}  {}", icons::PALETTE, palette.name)).strong();
    if entry.mine {
        row_with_stamp(ui, entry, clock, |ui| {
            ui.add(Label::new(name).truncate());
        });
        return;
    }
    ui.add(Label::new(name).truncate());
    ui.add_space(4.0);
    let owned = palettes.with_colors(&palette.colors).is_some();
    let text = if owned { owned_label() } else { add_label() };
    let needed = button_width(ui, &text) + TEXT_STAMP_GAP + stamp_width(ui, entry, clock);
    let mut add = |ui: &mut Ui| {
        if ui
            .add_enabled(!owned, Button::new(&text).wrap_mode(TextWrapMode::Extend))
            .clicked()
        {
            actions.push(Action::AddPalette(entry.id));
        }
    };
    if needed <= ui.available_width() {
        row_with_stamp(ui, entry, clock, add);
    } else {
        add(ui);
        stamp(ui, entry, clock);
    }
}

pub(super) fn card_width(ui: &Ui) -> f32 {
    PALETTE_WIDTH
        .max(button_width(ui, &owned_label()))
        .max(button_width(ui, &add_label()))
}

fn button_width(ui: &Ui, text: &str) -> f32 {
    measure(ui, RichText::new(text)) + 2.0 * ui.spacing().button_padding.x
}

fn owned_label() -> String {
    format!("{}  In your palettes", icons::CHECK)
}

fn add_label() -> String {
    format!("{}  Add to palettes", icons::PLUS)
}

pub(super) fn shared_palette(entry: &ChatEntry) -> Option<CustomPalette> {
    match &entry.body {
        citrine_gb_net::Body::Palette(json) if !entry.is_deleted() => {
            CustomPalette::from_shared(json)
        }
        _ => None,
    }
}

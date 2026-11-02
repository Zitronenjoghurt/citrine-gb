use super::DETAILS_MIN;
use crate::app::widgets::text::faint;
use citrine_gb_gamedata::GameState;
use egui::{Align, Color32, Layout, RichText, Sense, Stroke, Ui, vec2};

pub(super) fn beside(ui: &Ui, list_width: f32) -> bool {
    ui.available_width() >= list_width + 6.0 + DETAILS_MIN
}

pub(super) fn egg_details(ui: &mut Ui) {
    card(ui, false).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Egg").size(20.0).strong());
    });
}

pub(super) fn card(ui: &Ui, selected: bool) -> egui::Frame {
    card_with_margin(ui, selected, 10, 8)
}

pub(super) fn card_with_margin(ui: &Ui, selected: bool, x: i8, y: i8) -> egui::Frame {
    let visuals = ui.visuals();
    let normal = visuals.widgets.noninteractive.bg_stroke;
    let stroke = if selected {
        Stroke::new(2.0_f32, visuals.selection.stroke.color)
    } else {
        normal
    };
    let extra = (stroke.width - normal.width).round() as i8;
    egui::Frame::new()
        .fill(visuals.faint_bg_color)
        .stroke(stroke)
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(x - extra, y - extra))
}

pub(super) fn fact(ui: &mut Ui, label: &str, value: &str) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.add(egui::Label::new(faint(label)).extend());
        ui.add(egui::Label::new(value).extend());
    });
}

pub(super) fn text_width(ui: &Ui, text: RichText) -> f32 {
    egui::WidgetText::from(text)
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Body,
        )
        .size()
        .x
}

pub(super) fn fact_width(ui: &Ui, label: &str, value: &str) -> f32 {
    text_width(ui, RichText::new(label).small()).max(text_width(ui, RichText::new(value)))
}

pub(super) fn right_cell(ui: &mut Ui, width: f32, text: RichText) {
    ui.allocate_ui_with_layout(
        vec2(width, ui.spacing().interact_size.y),
        Layout::right_to_left(Align::Center),
        |ui| {
            ui.set_width(width);
            ui.label(text);
        },
    );
}

pub(super) fn chip(ui: &mut Ui, text: &str) {
    egui::Frame::new()
        .fill(ui.visuals().widgets.inactive.weak_bg_fill)
        .corner_radius(3.0)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(RichText::new(text).small()).truncate());
        });
}

pub(super) fn meter(ui: &mut Ui, fraction: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 5.0), Sense::hover());
    let painter = ui.painter();
    let track = if ui.visuals().dark_mode {
        ui.visuals().extreme_bg_color
    } else {
        ui.visuals().widgets.inactive.weak_bg_fill
    };
    painter.rect_filled(rect, 3.0, track);
    let mut fill = rect;
    fill.set_width(rect.width() * fraction.clamp(0.0, 1.0));
    painter.rect_filled(fill, 3.0, color);
}

pub(super) fn labelled_meter(ui: &mut Ui, label: &str, value: &str, fraction: f32, color: Color32) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).small());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(value).small());
        });
    });
    meter(ui, fraction, color);
}

pub(super) const BADGE_SIDE: f32 = 12.0;
pub(super) const BADGE_GAP: f32 = 6.0;

pub(super) fn badges_width(count: usize) -> f32 {
    count as f32 * (BADGE_SIDE + BADGE_GAP)
}

pub(super) fn badges(ui: &mut Ui, earned: &[bool]) {
    let side = BADGE_SIDE;
    let gap = BADGE_GAP;
    let width = badges_width(earned.len());
    let height = ui.text_style_height(&egui::TextStyle::Body);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let color = badge_color(ui);
    let off = ui.visuals().weak_text_color();
    for (i, on) in earned.iter().enumerate() {
        let center = egui::pos2(
            rect.left() + side / 2.0 + 2.0 + i as f32 * (side + gap),
            rect.center().y,
        );
        let r = side * 0.62;
        let points = vec![
            center + vec2(0.0, -r),
            center + vec2(r, 0.0),
            center + vec2(0.0, r),
            center + vec2(-r, 0.0),
        ];
        let shape = if *on {
            egui::Shape::convex_polygon(points, color, Stroke::NONE)
        } else {
            egui::Shape::convex_polygon(points, Color32::TRANSPARENT, Stroke::new(1.5_f32, off))
        };
        ui.painter().add(shape);
    }
}

pub(super) fn paint_caught_mark(painter: &egui::Painter, c: egui::Pos2) {
    painter.circle_filled(c, 5.0, Color32::WHITE);
    painter.add(egui::Shape::convex_polygon(
        (0..=16)
            .map(|i| {
                let a = std::f32::consts::PI * (1.0 + i as f32 / 16.0);
                c + vec2(a.cos(), a.sin()) * 5.0
            })
            .collect(),
        Color32::from_rgb(0xe0, 0x40, 0x40),
        Stroke::NONE,
    ));
    painter.circle_stroke(c, 5.0, Stroke::new(1.0_f32, Color32::from_gray(60)));
}

pub(super) fn tile_color(ui: &Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(0xe0, 0xe0, 0xd8)
    } else {
        Color32::from_rgb(0xef, 0xef, 0xef)
    }
}

pub(super) fn badge_color(ui: &Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(0xe3, 0xa5, 0x1a)
    } else {
        Color32::from_rgb(0xd9, 0x9a, 0x00)
    }
}

pub(super) fn exp_color(ui: &Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(0x4a, 0x9f, 0xd8)
    } else {
        Color32::from_rgb(0x2a, 0x7f, 0xc0)
    }
}

pub(super) fn hp_color(fraction: f32) -> Color32 {
    if fraction > 0.5 {
        Color32::from_rgb(0x3c, 0xa8, 0x46)
    } else if fraction > 0.2 {
        Color32::from_rgb(0xd8, 0x9a, 0x10)
    } else {
        Color32::from_rgb(0xd8, 0x40, 0x40)
    }
}

pub(super) fn type_color(id: u8) -> Color32 {
    let rgb = match id {
        0 => 0x9a9a6a,
        1 => 0xb83a30,
        2 => 0x8e7ad8,
        3 => 0x9a4a9a,
        4 => 0xc8a850,
        5 => 0xa89238,
        7 => 0x98a820,
        8 => 0x6a5890,
        9 => 0x9898b8,
        20 => 0xe07a30,
        21 => 0x5a88e0,
        22 => 0x68b048,
        23 => 0xd8b020,
        24 => 0xe0507a,
        25 => 0x78c8c8,
        26 => 0x6a40e0,
        27 => 0x6a5848,
        _ => 0x7a7a7a,
    };
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

pub(super) fn type_text_color(id: u8) -> Color32 {
    let fill = type_color(id);
    let linear = |c: u8| {
        let c = f32::from(c) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let luminance =
        0.2126 * linear(fill.r()) + 0.7152 * linear(fill.g()) + 0.0722 * linear(fill.b());
    let dark = Color32::from_rgb(0x1a, 0x1a, 0x1a);
    if 1.05 / (luminance + 0.05) >= (luminance + 0.05) / 0.06 {
        Color32::WHITE
    } else {
        dark
    }
}

pub(super) fn box_name(state: &GameState, index: usize) -> String {
    state
        .boxes
        .get(index)
        .and_then(|b| b.name.clone())
        .unwrap_or_else(|| format!("Box {}", index + 1))
}

pub(super) fn money(amount: u32) -> String {
    format!("¥{}", grouped(amount))
}

pub(super) fn grouped(value: u32) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

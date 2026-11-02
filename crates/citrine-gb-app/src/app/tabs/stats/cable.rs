use super::{fact_row, heading, play_time, sessions};
use crate::app::tabs::linked::LINK_ICON;
use crate::icons;
use crate::stats::{CablePartner, CableSum, CableTotals, cable_sum};
use egui::{RichText, Ui, vec2};

pub(super) fn has_cable(cable: &[CableTotals]) -> bool {
    cable.iter().any(|totals| totals.ms > 0)
}

pub(super) fn sum_text(sum: CableSum) -> String {
    format!("{} · {}", play_time(sum.ms), sessions(sum.sessions))
}

pub(super) fn kind(partner: &CablePartner) -> (&'static str, &'static str) {
    match partner {
        CablePartner::Friend(_) => (icons::USER, "Friend, online cable"),
        CablePartner::Game(_) => (icons::GAME_CONTROLLER, "Second game, local cable"),
        CablePartner::Event(_) => (icons::HANDSHAKE, "Event partner, local cable"),
    }
}

pub(super) fn name(totals: &CableTotals) -> &str {
    match (&totals.partner, totals.name.is_empty()) {
        (_, false) => &totals.name,
        (CablePartner::Friend(_), true) => "A friend",
        (CablePartner::Game(_), true) => "A second game",
        (CablePartner::Event(_), true) => "Event partner",
    }
}

pub(super) fn section(ui: &mut Ui, cable: &[CableTotals]) {
    if !has_cable(cable) {
        return;
    }
    ui.add_space(12.0);
    heading(ui, LINK_ICON, "Link cable");
    for (label, online) in [("Local cable", false), ("Online cable", true)] {
        let sum = cable_sum(cable, online);
        if sum.ms > 0 {
            fact_row(ui, label, sum_text(sum));
        }
    }
    ui.add_space(6.0);
    for totals in cable.iter().filter(|totals| totals.ms > 0) {
        partner_row(ui, totals);
    }
}

fn partner_row(ui: &mut Ui, totals: &CableTotals) {
    let (icon, what) = kind(&totals.partner);
    let line = ui.text_style_height(&egui::TextStyle::Body);
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(0, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.allocate_ui_with_layout(
                vec2(ui.available_width(), line),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.label(RichText::new(play_time(totals.ms)).strong());
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(icon);
                        ui.add(egui::Label::new(name(totals)).truncate());
                    });
                },
            );
            ui.add(
                egui::Label::new(
                    RichText::new(format!("{what} · {}", sessions(totals.sessions))).weak(),
                )
                .truncate(),
            );
        });
}

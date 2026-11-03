use crate::icons;
use crate::utils::file_loader::PickedFile;
use crate::utils::file_saver::SaveOutcome;
use crate::your_data::{Change, Files, Kind, Tally};
use egui::{Align, Layout, RichText, Ui, vec2};
use std::sync::mpsc::{Receiver, Sender, channel};

pub const EXPORT: &str = "Export";
pub const IMPORT: &str = "Import";
pub const DELETE: &str = "Delete";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Export,
    Import,
    Delete,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Exporting {
    #[default]
    Idle,
    Busy,
    Done,
}

pub struct PendingImport {
    pub name: String,
    pub files: Files,
    pub plan: Vec<(Kind, Change)>,
}

pub struct YourDataUi {
    pub rows: Vec<(Kind, Tally)>,
    pub counted_at: Option<web_time::Instant>,
    pub request: Option<Request>,
    pub exporting: Exporting,
    pub deleting: bool,
    pub pending: Option<PendingImport>,
    pub files_tx: Sender<Files>,
    pub files_rx: Receiver<Files>,
    pub saved_tx: Sender<SaveOutcome>,
    pub saved_rx: Receiver<SaveOutcome>,
    pub picked_tx: Sender<PickedFile>,
    pub picked_rx: Receiver<PickedFile>,
}

impl Default for YourDataUi {
    fn default() -> Self {
        let (files_tx, files_rx) = channel();
        let (saved_tx, saved_rx) = channel();
        let (picked_tx, picked_rx) = channel();
        Self {
            rows: Vec::new(),
            counted_at: None,
            request: None,
            exporting: Exporting::Idle,
            deleting: false,
            pending: None,
            files_tx,
            files_rx,
            saved_tx,
            saved_rx,
            picked_tx,
            picked_rx,
        }
    }
}

const ROW_GAP: f32 = 8.0;
const BUTTON_HEIGHT: f32 = 32.0;

pub fn show(ui: &mut Ui, state: &mut YourDataUi) {
    ui.with_layout(Layout::top_down(Align::Min), |ui| {
        rows(ui, &state.rows);
        ui.add_space(12.0);
        buttons(ui, state);
    });
}

fn rows(ui: &mut Ui, rows: &[(Kind, Tally)]) {
    let body = egui::TextStyle::Body.resolve(ui.style());
    let measure = |text: String, font: egui::FontId| {
        ui.ctx().fonts_mut(|f| {
            f.layout_no_wrap(text, font, egui::Color32::PLACEHOLDER)
                .size()
                .x
        })
    };
    let count_width = rows
        .iter()
        .map(|(kind, tally)| measure(count_text(kind, tally.count), body.clone()))
        .fold(0.0, f32::max);
    let size_width = rows
        .iter()
        .map(|(_, tally)| measure(size_text(tally), body.clone()))
        .fold(measure("999 KB".to_owned(), body.clone()), f32::max);
    let height = ui.spacing().interact_size.y;
    for (kind, tally) in rows {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let name_width =
                (ui.available_width() - count_width - size_width - 2.0 * ROW_GAP).max(0.0);
            ui.allocate_ui_with_layout(
                vec2(name_width, height),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.set_width(name_width);
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.label(kind.icon);
                    ui.add(egui::Label::new(kind.name).truncate());
                },
            );
            ui.add_space(ROW_GAP);
            ui.allocate_ui_with_layout(
                vec2(count_width, height),
                Layout::right_to_left(Align::Center),
                |ui| {
                    ui.set_width(count_width);
                    ui.label(count_text(kind, tally.count));
                },
            );
            ui.add_space(ROW_GAP);
            ui.allocate_ui_with_layout(
                vec2(size_width, height),
                Layout::right_to_left(Align::Center),
                |ui| {
                    ui.set_width(size_width);
                    ui.label(RichText::new(size_text(tally)).weak());
                },
            );
        });
    }
}

pub fn count_text(kind: &Kind, count: usize) -> String {
    kind.unit(count)
}

pub fn size_text(tally: &Tally) -> String {
    crate::media::format_size(if tally.count == 0 { 0 } else { tally.bytes })
}

fn buttons(ui: &mut Ui, state: &mut YourDataUi) {
    let busy = state.exporting == Exporting::Busy;
    let export = if busy {
        format!("{} Exporting…", icons::HOURGLASS)
    } else {
        format!("{} {EXPORT}", icons::DOWNLOAD_SIMPLE)
    };
    let labels = [
        (export, !busy, Request::Export),
        (
            format!("{} {IMPORT}", icons::UPLOAD_SIMPLE),
            true,
            Request::Import,
        ),
        (format!("{} {DELETE}", icons::TRASH), true, Request::Delete),
    ];
    let spacing = ui.spacing().item_spacing.x;
    let padding = 2.0 * ui.spacing().button_padding.x;
    let available = ui.available_width();
    let share = ((available - 2.0 * spacing) / 3.0).max(0.0);
    let widest = labels
        .iter()
        .map(|(text, _, _)| {
            egui::WidgetText::from(text.as_str())
                .into_galley(
                    ui,
                    Some(egui::TextWrapMode::Extend),
                    f32::INFINITY,
                    egui::TextStyle::Button,
                )
                .size()
                .x
                + padding
        })
        .fold(0.0, f32::max);
    let mut press = |ui: &mut Ui, width: f32| {
        for (text, enabled, request) in &labels {
            let button = egui::Button::new(text.as_str()).min_size(vec2(width, BUTTON_HEIGHT));
            if ui.add_enabled(*enabled, button).clicked() {
                state.request = Some(*request);
            }
        }
    };
    if widest <= share {
        ui.horizontal(|ui| press(ui, share));
    } else {
        press(ui, available);
    }
}

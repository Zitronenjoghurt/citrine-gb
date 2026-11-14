use crate::app::widgets::progress::ProgressBar;
use crate::app::widgets::settings::form::Form;
use crate::app::widgets::settings::page::Page;
use crate::icons;
use crate::progress::{Counter, Progress};
use crate::utils::file_loader::PickedFile;
use crate::utils::file_saver::SaveOutcome;
use crate::your_data::{Change, Files, Kind, Tally};
use egui::{Align, Layout, RichText, Ui, vec2};
use std::sync::mpsc::{Receiver, Sender, channel};

pub const EXPORT: &str = "Export";
pub const IMPORT: &str = "Import";
pub const DELETE: &str = "Delete everything";
pub const DELETE_SHORT: &str = "Delete";

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

pub type Unpacked = (String, Result<Files, crate::your_data::ReadError>);

pub struct YourDataUi {
    pub rows: Vec<(Kind, Tally)>,
    pub counted_at: Option<web_time::Instant>,
    pub request: Option<Request>,
    pub exporting: Exporting,
    pub deleting: bool,
    pub pending: Option<PendingImport>,
    pub progress: Counter,
    pub importing: Option<String>,
    pub packed_tx: Sender<std::io::Result<Vec<u8>>>,
    pub packed_rx: Receiver<std::io::Result<Vec<u8>>>,
    pub unpacked_tx: Sender<Unpacked>,
    pub unpacked_rx: Receiver<Unpacked>,
    pub saved_tx: Sender<SaveOutcome>,
    pub saved_rx: Receiver<SaveOutcome>,
    pub picked_tx: Sender<PickedFile>,
    pub picked_rx: Receiver<PickedFile>,
}

impl Default for YourDataUi {
    fn default() -> Self {
        let (packed_tx, packed_rx) = channel();
        let (unpacked_tx, unpacked_rx) = channel();
        let (saved_tx, saved_rx) = channel();
        let (picked_tx, picked_rx) = channel();
        Self {
            rows: Vec::new(),
            counted_at: None,
            request: None,
            exporting: Exporting::Idle,
            deleting: false,
            pending: None,
            progress: Counter::default(),
            importing: None,
            packed_tx,
            packed_rx,
            unpacked_tx,
            unpacked_rx,
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
    Page::show(ui, |page| {
        page.plain(|ui| {
            Form::show(ui, "your_data_rows", |form| {
                form.custom(|ui| rows(ui, &state.rows));
            })
        });
        page.plain(|ui| {
            Form::show(ui, "your_data_buttons", |form| {
                form.custom(|ui| buttons(ui, state));
                if let Some((label, progress)) = load(state) {
                    form.custom(|ui| ui.add(ProgressBar::new(&label, progress)));
                }
            })
        });
    });
}

pub fn loading(state: &YourDataUi) -> bool {
    state.exporting == Exporting::Busy || state.importing.is_some()
}

pub fn load(state: &YourDataUi) -> Option<(String, Progress)> {
    let progress = state.progress.bytes();
    if let Some(name) = &state.importing {
        return Some((format!("Reading '{name}'"), progress));
    }
    (state.exporting == Exporting::Busy && !progress.is_done())
        .then(|| ("Exporting your data".to_owned(), progress))
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
    let size_width = rows
        .iter()
        .map(|(_, tally)| measure(size_text(tally), body.clone()))
        .fold(measure("999 KB".to_owned(), body.clone()), f32::max);
    let counts: Vec<(String, f32)> = rows
        .iter()
        .map(|(kind, tally)| {
            let count = count_text(kind, tally.count);
            let width = measure(count.clone(), body.clone());
            (count, width)
        })
        .collect();
    let height = ui.spacing().interact_size.y;
    for ((kind, tally), (count, count_width)) in rows.iter().zip(counts) {
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
                    ui.label(count);
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
    let busy = loading(state);
    let spacing = ui.spacing().item_spacing.x;
    let padding = 2.0 * ui.spacing().button_padding.x;
    let available = ui.available_width();
    let share = ((available - 2.0 * spacing) / 3.0).max(0.0);
    let width = |text: &str| {
        egui::WidgetText::from(text)
            .into_galley(
                ui,
                Some(egui::TextWrapMode::Extend),
                f32::INFINITY,
                egui::TextStyle::Button,
            )
            .size()
            .x
            + padding
    };
    let tiers =
        [(true, DELETE), (true, DELETE_SHORT), (false, DELETE_SHORT)].map(|(icon, delete)| {
            [
                (icons::DOWNLOAD_SIMPLE, EXPORT),
                (icons::UPLOAD_SIMPLE, IMPORT),
                (icons::TRASH, delete),
            ]
            .map(|(mark, label)| {
                if icon {
                    format!("{mark} {label}")
                } else {
                    label.to_owned()
                }
            })
        });
    let fitting = tiers
        .iter()
        .find(|tier| tier.iter().all(|text| width(text) <= share))
        .cloned();
    let fits = fitting.is_some();
    let [export, import, delete] = fitting.unwrap_or_else(|| tiers[0].clone());
    let labels = [
        (export, !busy, Request::Export),
        (import, !busy, Request::Import),
        (delete, true, Request::Delete),
    ];
    let mut press = |ui: &mut Ui, width: f32| {
        for (text, enabled, request) in &labels {
            let button = egui::Button::new(text.as_str()).min_size(vec2(width, BUTTON_HEIGHT));
            if ui.add_enabled(*enabled, button).clicked() {
                state.request = Some(*request);
            }
        }
    };
    if fits {
        ui.horizontal(|ui| press(ui, share));
    } else {
        press(ui, available);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(name: &'static str, units: (&'static str, &'static str)) -> Kind {
        Kind {
            name,
            icon: icons::TAG,
            units,
            nouns: units,
        }
    }

    #[test]
    fn a_short_count_leaves_its_name_whole_in_the_dock() {
        let ctx = egui::Context::default();
        let table = [
            (
                kind("Media", ("", "")),
                Tally {
                    count: 600,
                    bytes: 26 << 20,
                },
            ),
            (kind("Chats", ("message", "messages")), Tally::default()),
            (kind("Game names", ("", "")), Tally::default()),
        ];
        let mut elided = Vec::new();
        for _ in 0..2 {
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.allocate_ui(vec2(217.0, 400.0), |ui| rows(ui, &table));
                });
            });
            elided = output
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    egui::Shape::Text(text) if text.galley.elided => {
                        Some(text.galley.text().to_owned())
                    }
                    _ => None,
                })
                .collect();
        }
        assert!(elided.is_empty(), "cut short: {elided:?}");
    }

    #[test]
    fn the_default_dock_keeps_the_three_buttons_in_one_row() {
        let ctx = egui::Context::default();
        let mut state = YourDataUi::default();
        let mut tops = Vec::new();
        for _ in 0..2 {
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.allocate_ui(vec2(201.0, 400.0), |ui| buttons(ui, &mut state));
                });
            });
            tops = output
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    egui::Shape::Text(text) => Some(text.pos.y),
                    _ => None,
                })
                .collect();
        }
        assert_eq!(tops.len(), 3);
        assert!(
            tops.iter().all(|top| (top - tops[0]).abs() < 0.5),
            "{tops:?}"
        );
    }
}

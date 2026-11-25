use crate::app::widgets::progress::ProgressBar;
use crate::app::widgets::settings::form::Form;
use crate::icons;
use crate::progress::{Counter, Progress};
use crate::utils::file_loader::PickedFile;
use crate::utils::file_saver::SaveOutcome;
use crate::your_data::{Change, Files, Kind, Tally};
use egui::{Ui, vec2};
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

pub const BUTTON_HEIGHT: f32 = 32.0;

pub fn actions(form: &mut Form, state: &mut YourDataUi) {
    form.custom(|ui| buttons(ui, state));
    if let Some((label, progress)) = load(state) {
        form.custom(|ui| ui.add(ProgressBar::new(&label, progress)));
    }
}

pub fn loading(state: &YourDataUi) -> bool {
    state.exporting == Exporting::Busy || state.importing.is_some()
}

pub fn count(state: &YourDataUi, kind: crate::storage::kinds::Kind) -> Option<usize> {
    state.counted_at?;
    Some(
        state
            .rows
            .iter()
            .filter(|(row, _)| row.store == kind)
            .map(|(_, tally)| tally.count)
            .sum(),
    )
}

pub fn nothing_stored(state: &YourDataUi) -> bool {
    state.counted_at.is_some() && state.rows.iter().all(|(_, tally)| tally.count == 0)
}

pub fn load(state: &YourDataUi) -> Option<(String, Progress)> {
    let progress = state.progress.bytes();
    if let Some(name) = &state.importing {
        return Some((format!("Reading '{name}'"), progress));
    }
    (state.exporting == Exporting::Busy && !progress.is_done())
        .then(|| ("Exporting your data".to_owned(), progress))
}

fn buttons(ui: &mut Ui, state: &mut YourDataUi) {
    let busy = loading(state);
    let empty = nothing_stored(state);
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
    let labelled = |mark: &str, label: &str, icon: bool| {
        if icon {
            format!("{mark} {label}")
        } else {
            label.to_owned()
        }
    };
    let pick = |label: &str, mark: &str, room: f32| {
        let full = labelled(mark, label, true);
        if width(&full) <= room {
            full
        } else {
            label.to_owned()
        }
    };
    let pair_share = ((available - spacing) / 2.0).max(0.0);
    let three_fit = [true, false].into_iter().any(|icon| {
        [
            labelled(icons::DOWNLOAD_SIMPLE, EXPORT, icon),
            labelled(icons::UPLOAD_SIMPLE, IMPORT, icon),
            labelled(icons::TRASH, DELETE, icon),
        ]
        .iter()
        .all(|text| width(text) <= share)
    });
    let (columns, cell) = if three_fit {
        (3, share)
    } else if [EXPORT, IMPORT]
        .iter()
        .all(|text| width(text) <= pair_share)
    {
        (2, pair_share)
    } else {
        (1, available)
    };
    let room = if columns == 3 { share } else { available };
    let danger = crate::app::ui_state::ui_theme::danger_text(ui.visuals());
    let labels = [
        (
            pick(EXPORT, icons::DOWNLOAD_SIMPLE, cell),
            !busy && !empty,
            Request::Export,
        ),
        (
            pick(IMPORT, icons::UPLOAD_SIMPLE, cell),
            !busy,
            Request::Import,
        ),
        (pick(DELETE, icons::TRASH, room), !empty, Request::Delete),
    ];
    let mut press =
        |ui: &mut Ui, (text, enabled, request): &(String, bool, Request), width: f32| {
            let text = egui::RichText::new(text.as_str());
            let text = if *request == Request::Delete {
                text.color(danger)
            } else {
                text
            };
            let button = egui::Button::new(text).min_size(vec2(width, BUTTON_HEIGHT));
            if ui.add_enabled(*enabled, button).clicked() {
                state.request = Some(*request);
            }
        };
    match columns {
        3 => {
            ui.horizontal(|ui| {
                for label in &labels {
                    press(ui, label, cell);
                }
            });
        }
        2 => {
            ui.horizontal(|ui| {
                for label in &labels[..2] {
                    press(ui, label, cell);
                }
            });
            ui.vertical_centered(|ui| press(ui, &labels[2], available));
        }
        _ => {
            ui.vertical_centered(|ui| {
                for label in &labels {
                    press(ui, label, available);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_everything_is_the_phone_red_in_both_themes() {
        use crate::app::ui_state::ui_theme::UiTheme;
        for (theme, ui_theme, red) in [
            (
                egui::Theme::Dark,
                UiTheme::Dark,
                egui::Color32::from_rgb(255, 150, 150),
            ),
            (
                egui::Theme::Light,
                UiTheme::Light,
                egui::Color32::from_rgb(190, 20, 20),
            ),
        ] {
            let ctx = egui::Context::default();
            ctx.set_theme(theme);
            ui_theme.apply(&ctx);
            let mut state = YourDataUi::default();
            let mut colours = Vec::new();
            for _ in 0..2 {
                let output = ctx.run(egui::RawInput::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.allocate_ui(vec2(201.0, 400.0), |ui| buttons(ui, &mut state));
                    });
                });
                colours = output
                    .shapes
                    .into_iter()
                    .filter_map(|clipped| match clipped.shape {
                        egui::Shape::Text(text) if text.galley.text().ends_with(DELETE) => Some(
                            text.galley
                                .job
                                .sections
                                .iter()
                                .map(|section| section.format.color)
                                .collect::<Vec<_>>(),
                        ),
                        _ => None,
                    })
                    .flatten()
                    .collect();
            }
            assert!(!colours.is_empty(), "{theme:?}");
            assert!(
                colours.iter().all(|colour| *colour == red),
                "{theme:?} {colours:?}"
            );
        }
    }

    #[test]
    fn the_default_dock_gives_delete_everything_its_own_row() {
        let ctx = egui::Context::default();
        let mut state = YourDataUi::default();
        let mut texts = Vec::new();
        for _ in 0..2 {
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.allocate_ui(vec2(201.0, 400.0), |ui| buttons(ui, &mut state));
                });
            });
            texts = output
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    egui::Shape::Text(text) => Some((
                        text.galley.text().to_owned(),
                        text.pos.y,
                        text.pos.x + text.galley.size().x / 2.0,
                    )),
                    _ => None,
                })
                .collect();
        }
        assert_eq!(texts.len(), 3, "{texts:?}");
        assert!((texts[0].1 - texts[1].1).abs() < 0.5, "{texts:?}");
        assert!(texts[2].1 > texts[0].1 + 10.0, "{texts:?}");
        assert!(texts[2].0.ends_with(DELETE), "{texts:?}");
        let row_middle = (texts[0].2 + texts[1].2) / 2.0;
        assert!((texts[2].2 - row_middle).abs() < 1.0, "{texts:?}");
    }
}

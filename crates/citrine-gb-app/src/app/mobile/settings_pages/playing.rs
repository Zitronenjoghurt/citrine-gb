use super::{Sheet, label, opens, pick, settled, slide};
use crate::app::Citrine;
use crate::app::mobile::kit::{self, Row};
use crate::app::mobile::sheet::Amount;
use crate::app::ui_state::settings_tree::SettingsTab;
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::autosave::{DEFAULT_KEEP, MAX_KEEP};
use crate::emulator::rewind::REWIND_NOTE;
use egui::{Context, Ui};

const AUTOSAVE_NOTE: &str = "Minutes of play between autosaves. Paused time doesn't count. Citrine also autosaves when you leave or switch games, and after a crash.";

fn count(value: f64) -> String {
    format!("{value:.0}")
}

fn saves(count: usize) -> String {
    match count {
        1 => "1 save".to_owned(),
        n => format!("{n} saves"),
    }
}

impl Citrine {
    pub(super) fn playing_page(&mut self, ui: &mut Ui) {
        self.tree_page(
            ui,
            SettingsTab::Playing,
            |_| None,
            Self::playing_row,
            |_| {},
        );
    }

    fn playing_row(&mut self, list: &mut kit::List<'_>, row: RowId) -> Option<Sheet> {
        let s = &self.ui.settings;
        match row {
            RowId::FastForward => opens(
                list,
                Row::new(label(row)).value(s.fast_forward_speed.to_string()),
                Sheet::FastForward,
            ),
            RowId::FastForwardKey => opens(
                list,
                Row::new(label(row)).value(s.fast_forward_key.to_string()),
                Sheet::FastForwardKey,
            ),
            RowId::QuickLoad => opens(
                list,
                Row::new(label(row)).value(s.quick_load.to_string()),
                Sheet::QuickLoad,
            ),
            RowId::InBackground => opens(
                list,
                Row::new(label(row)).value(s.background_pause.to_string()),
                Sheet::InBackground,
            ),
            RowId::Rewind => opens(
                list,
                Row::new(label(row))
                    .value(s.rewind.to_string())
                    .info(REWIND_NOTE),
                Sheet::Rewind,
            ),
            RowId::AutosaveEvery => opens(
                list,
                Row::new(label(row)).value(s.autosave_interval.to_string()),
                Sheet::AutosaveEvery,
            ),
            RowId::AutosavesKept => opens(
                list,
                Row::new(label(row)).value(saves(s.autosave_keep)),
                Sheet::AutosaveKeep,
            ),
            _ => None,
        }
    }

    pub(super) fn playing_sheet(&mut self, ctx: &Context, open: Sheet) -> bool {
        let s = &mut self.ui.settings;
        let (changed, done) = match open {
            Sheet::AutosaveEvery => pick(
                ctx,
                label(RowId::AutosaveEvery),
                Some(AUTOSAVE_NOTE),
                &mut s.autosave_interval,
            ),
            Sheet::AutosaveKeep => {
                let shown = slide(
                    ctx,
                    label(RowId::AutosavesKept),
                    Some("When there are more, the oldest autosave goes."),
                    Amount {
                        value: &mut s.autosave_keep,
                        range: 1..=MAX_KEEP,
                        step: 1.0,
                        default: DEFAULT_KEEP,
                        text: count,
                    },
                );
                (settled(&shown.inner), shown.closed)
            }
            Sheet::FastForward => pick(
                ctx,
                label(RowId::FastForward),
                None,
                &mut s.fast_forward_speed,
            ),
            Sheet::FastForwardKey => pick(
                ctx,
                label(RowId::FastForwardKey),
                None,
                &mut s.fast_forward_key,
            ),
            Sheet::QuickLoad => pick(ctx, label(RowId::QuickLoad), None, &mut s.quick_load),
            Sheet::InBackground => pick(
                ctx,
                label(RowId::InBackground),
                None,
                &mut s.background_pause,
            ),
            _ => pick(ctx, label(RowId::Rewind), Some(REWIND_NOTE), &mut s.rewind),
        };
        s.dirty |= changed;
        done
    }
}

use super::{Sheet, label, opens, slide};
use crate::app::Citrine;
use crate::app::hud::DEFAULT_OPACITY;
use crate::app::hud::editor::CARD_HINT;
use crate::app::mobile::kit::{self, Row, SwitchRow};
use crate::app::mobile::sheet::{self, Amount};
use crate::app::ui_state::settings::{
    GHOSTING_NOTE, GRID_NOTE, MATCH_SCREEN_NOTE, SAME_SIZE_PIXELS_NOTE, ScalingMode, Settings,
    grid_note,
};
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::ui_state::settings_tree::{SettingsTab, Surface};
use egui::{Context, Ui};

impl Citrine {
    pub(super) fn screen_page(&mut self, ui: &mut Ui) {
        self.tree_page(ui, SettingsTab::Screen, |_| None, Self::screen_row, |_| {});
    }

    fn screen_row(&mut self, list: &mut kit::List<'_>, row: RowId) -> Option<Sheet> {
        let s = &mut self.ui.settings;
        let integer = s.touch.scaling == ScalingMode::Integer;
        let mut dirty = false;
        let open = match row {
            RowId::Scaling => opens(
                list,
                Row::new(label(row)).value(s.touch.scaling.to_string()),
                Sheet::TouchScaling,
            ),
            RowId::SameSizePixels if !integer => {
                dirty |= list.switch(
                    SwitchRow::new(label(row), &mut s.same_size_pixels).info(SAME_SIZE_PIXELS_NOTE),
                );
                None
            }
            RowId::MatchRefresh => {
                dirty |= list.switch(
                    SwitchRow::new(label(row), &mut s.match_screen).info(MATCH_SCREEN_NOTE),
                );
                None
            }
            RowId::LcdGrid => {
                let forced = s.matrix && !integer && !s.same_size_pixels;
                let note = if forced {
                    grid_note(s.touch.scaling, Surface::Phone)
                } else {
                    GRID_NOTE.to_owned()
                };
                dirty |= list.switch(SwitchRow::new(label(row), &mut s.matrix).info(&note));
                None
            }
            RowId::GridLines if s.matrix => opens(
                list,
                Row::new(label(row)).value(sheet::percent(s.matrix_edge_darkness.into())),
                Sheet::EdgeDarkness,
            ),
            RowId::GridCorners if s.matrix => opens(
                list,
                Row::new(label(row)).value(sheet::percent(s.matrix_corner_darkness.into())),
                Sheet::CornerDarkness,
            ),
            RowId::Ghosting => {
                dirty |=
                    list.switch(SwitchRow::new(label(row), &mut s.ghosting).info(GHOSTING_NOTE));
                None
            }
            RowId::GhostingStrength if s.ghosting => opens(
                list,
                Row::new(label(row)).value(sheet::percent(s.ghosting_strength.into())),
                Sheet::GhostStrength,
            ),
            RowId::ShowHuds => {
                dirty |= list.switch(SwitchRow::new(label(row), &mut s.hud.shown));
                None
            }
            RowId::HudOpacity => opens(
                list,
                Row::new(label(row))
                    .value(sheet::percent(s.hud.opacity().into()))
                    .info(CARD_HINT),
                Sheet::HudCard,
            ),
            _ => None,
        };
        s.dirty |= dirty;
        open
    }

    pub(super) fn screen_sheet(&mut self, ctx: &Context, open: Sheet) -> bool {
        let s = &mut self.ui.settings;
        let (value, default, row) = match open {
            Sheet::EdgeDarkness => (
                &mut s.matrix_edge_darkness,
                Settings::DEFAULT_MATRIX_EDGE_DARKNESS,
                RowId::GridLines,
            ),
            Sheet::CornerDarkness => (
                &mut s.matrix_corner_darkness,
                Settings::DEFAULT_MATRIX_CORNER_DARKNESS,
                RowId::GridCorners,
            ),
            Sheet::GhostStrength => (
                &mut s.ghosting_strength,
                Settings::DEFAULT_GHOSTING_STRENGTH,
                RowId::GhostingStrength,
            ),
            _ => {
                let shown = slide(
                    ctx,
                    label(RowId::HudOpacity),
                    Some(CARD_HINT),
                    Amount {
                        value: &mut s.hud.opacity,
                        range: 0.0..=1.0,
                        step: 0.05,
                        default: DEFAULT_OPACITY,
                        text: sheet::percent,
                    },
                );
                s.dirty |= shown.inner.changed();
                return shown.closed;
            }
        };
        let shown = slide(
            ctx,
            label(row),
            None,
            Amount {
                value,
                range: 0.0..=1.0,
                step: 0.01,
                default,
                text: sheet::percent,
            },
        );
        s.dirty |= shown.inner.changed();
        shown.closed
    }
}

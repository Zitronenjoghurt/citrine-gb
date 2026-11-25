use super::{Sheet, label, opens, pick, pick_from};
use crate::app::Citrine;
use crate::app::mobile::kit::{self, Row};
use crate::app::ui_state::settings_tree::SettingsTab;
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::emulator::rewind::CLIP_LENGTH_NOTE;
use crate::media::recording::MAX_SCALE;
use egui::{Context, Ui};

fn scale_label(scale: usize) -> String {
    format!("{scale}x")
}

impl Citrine {
    pub(super) fn capture_page(&mut self, ui: &mut Ui) {
        self.tree_page(
            ui,
            SettingsTab::Capture,
            |_| None,
            Self::capture_row,
            |_| {},
        );
    }

    fn capture_row(&mut self, list: &mut kit::List<'_>, row: RowId) -> Option<Sheet> {
        let s = &self.ui.settings;
        match row {
            RowId::ScreenshotSize => opens(
                list,
                Row::new(label(row)).value(scale_label(s.screenshot_scale)),
                Sheet::ScreenshotScale,
            ),
            RowId::RecordingSize => opens(
                list,
                Row::new(label(row)).value(scale_label(s.recording_scale)),
                Sheet::RecordingScale,
            ),
            RowId::RenderVideo => opens(
                list,
                Row::new(label(row)).value(s.render_video.to_string()),
                Sheet::RenderVideo,
            ),
            RowId::ClipLength => opens(
                list,
                Row::new(label(row))
                    .value(s.clip_length.to_string())
                    .info(CLIP_LENGTH_NOTE),
                Sheet::ClipLength,
            ),
            _ => None,
        }
    }

    pub(super) fn capture_sheet(&mut self, ctx: &Context, open: Sheet) -> bool {
        let s = &mut self.ui.settings;
        let (changed, done) = match open {
            Sheet::ScreenshotScale => pick_from(
                ctx,
                label(RowId::ScreenshotSize),
                None,
                1..=MAX_SCALE,
                &mut s.screenshot_scale,
                scale_label,
            ),
            Sheet::RenderVideo => pick(ctx, label(RowId::RenderVideo), None, &mut s.render_video),
            Sheet::ClipLength => pick(
                ctx,
                label(RowId::ClipLength),
                Some(CLIP_LENGTH_NOTE),
                &mut s.clip_length,
            ),
            _ => pick_from(
                ctx,
                label(RowId::RecordingSize),
                None,
                (2..=MAX_SCALE).step_by(2),
                &mut s.recording_scale,
                scale_label,
            ),
        };
        s.dirty |= changed;
        done
    }
}

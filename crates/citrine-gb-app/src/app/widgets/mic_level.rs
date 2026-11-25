use crate::app::ui_state::ui_theme;
use crate::audio::mic::MicTest;
use egui::{
    Align, Label, Layout, Rect, Response, RichText, Sense, TextStyle, Ui, Widget, WidgetInfo, pos2,
    vec2,
};

pub const LEVEL_LABEL: &str = "Microphone level";

const SEGMENTS: usize = 12;
const SEGMENT_GAP: f32 = 2.0;
pub const METER_HEIGHT: f32 = 4.0;

fn lit_segments(fraction: f32) -> usize {
    (fraction.clamp(0.0, 1.0) * SEGMENTS as f32).round() as usize
}

fn meter(ui: &mut Ui, fraction: f32) {
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), METER_HEIGHT), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let visuals = ui.visuals();
    let lit = ui_theme::meter(visuals);
    let unlit = crate::app::mobile::bar_track(ui).unwrap_or_else(|| ui_theme::track(visuals));
    let step = (rect.width() + SEGMENT_GAP) / SEGMENTS as f32;
    let on = lit_segments(fraction);
    for index in 0..SEGMENTS {
        let left = rect.left() + index as f32 * step;
        let segment = Rect::from_min_max(
            pos2(left, rect.top()),
            pos2(left + step - SEGMENT_GAP, rect.bottom()),
        );
        let fill = if index < on { lit } else { unlit };
        ui.painter().rect_filled(segment, 1.0, fill);
    }
}

pub struct MicLevel(pub MicTest);

impl Widget for MicLevel {
    fn ui(self, ui: &mut Ui) -> Response {
        let size = vec2(ui.available_width(), ui.text_style_height(&TextStyle::Body));
        let response = ui
            .allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| {
                ui.set_min_size(size);
                match self.0.words() {
                    Some(words) => {
                        ui.add(Label::new(RichText::new(words).weak()).wrap());
                    }
                    None => {
                        meter(ui, self.0.fraction());
                    }
                }
            })
            .response;
        let value = f64::from(self.0.fraction());
        let words = self.0.words();
        response.widget_info(|| {
            let mut info =
                WidgetInfo::labeled(egui::WidgetType::ProgressIndicator, true, LEVEL_LABEL);
            info.value = Some(value);
            info.current_text_value = words.map(str::to_owned);
            info
        });
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_meter_lights_no_segment_in_silence_and_all_at_full_level() {
        assert_eq!(lit_segments(0.0), 0);
        assert_eq!(lit_segments(0.01), 0);
        assert_eq!(lit_segments(0.5), SEGMENTS / 2);
        assert_eq!(lit_segments(1.0), SEGMENTS);
        assert_eq!(lit_segments(3.0), SEGMENTS);
    }

    #[test]
    fn a_silent_meter_stands_out_from_the_panel_in_both_themes() {
        use crate::app::ui_state::ui_theme::{TRACK_CONTRAST, UiTheme, contrast};
        use strum::IntoEnumIterator;
        for theme in UiTheme::iter() {
            let ctx = egui::Context::default();
            theme.apply(&ctx);
            let shapes = ctx
                .run(egui::RawInput::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| meter(ui, 0.0));
                })
                .shapes;
            let panel = ctx.style().visuals.panel_fill;
            let fills: Vec<_> = shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    egui::Shape::Rect(rect) if (rect.rect.height() - METER_HEIGHT).abs() < 0.01 => {
                        Some(panel.blend(rect.fill))
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(fills.len(), SEGMENTS, "{theme}");
            for fill in fills {
                assert!(
                    contrast(fill, panel) >= TRACK_CONTRAST,
                    "{theme}: unlit {fill:?} on {panel:?}"
                );
            }
        }
    }
}

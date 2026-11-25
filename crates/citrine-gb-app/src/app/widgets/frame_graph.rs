use crate::app::mobile::touch_square;
use crate::app::widgets::performance::budget_text;
use crate::frame_log::{Budget, FrameSample, SHOWN};
use egui::{Color32, Label, Response, RichText, Sense, Shape, Stroke, TextStyle, Ui, Widget, vec2};
use egui_plot::{HLine, Line, LineStyle, MarkerShape, Plot, PlotBounds, PlotPoints, Points};
use std::collections::VecDeque;
use std::time::Duration;

const DESKTOP_HEIGHT: f32 = 130.0;
const PHONE_HEIGHT: f32 = 150.0;
pub(crate) const GAME: (Color32, Color32) = (
    Color32::from_rgb(110, 170, 255),
    Color32::from_rgb(30, 100, 200),
);
pub(crate) const GPU: (Color32, Color32) = (
    Color32::from_rgb(190, 140, 255),
    Color32::from_rgb(120, 60, 200),
);

pub(crate) fn pick((on_dark, on_light): (Color32, Color32), dark: bool) -> Color32 {
    if dark { on_dark } else { on_light }
}

pub struct FrameGraph<'a> {
    pub samples: &'a VecDeque<FrameSample>,
    pub budget: Budget,
    pub gpu: bool,
}

pub struct Series {
    pub shown: Vec<[f64; 2]>,
    pub game: Vec<[f64; 2]>,
    pub gpu: Vec<[f64; 2]>,
    pub repeated: Vec<[f64; 2]>,
    pub top: f64,
}

fn ms(time: Duration) -> f64 {
    time.as_secs_f64() * 1000.0
}

pub fn series(samples: &VecDeque<FrameSample>, game_frame: Duration) -> Series {
    let newest = samples.back().map_or(Duration::ZERO, |sample| sample.at);
    let x = |sample: &FrameSample| -(newest.saturating_sub(sample.at)).as_secs_f64();
    let mut shown = Vec::new();
    let mut repeated = Vec::new();
    let mut since = Duration::ZERO;
    let mut held = false;
    let mut first = true;
    for sample in samples {
        since += sample.display;
        held |= sample.repeated > 0;
        if sample.game_frames == 0 {
            continue;
        }
        if !first {
            let point = [x(sample), ms(since)];
            shown.push(point);
            if held {
                repeated.push(point);
            }
        }
        first = false;
        since = Duration::ZERO;
        held = false;
    }
    let game: Vec<[f64; 2]> = samples
        .iter()
        .filter_map(|sample| Some([x(sample), ms(sample.game_per_frame()?)]))
        .collect();
    let gpu: Vec<[f64; 2]> = samples
        .iter()
        .filter_map(|sample| Some([x(sample), ms(sample.gpu?)]))
        .collect();
    let frame = ms(game_frame);
    let highest = shown
        .iter()
        .chain(&game)
        .chain(&gpu)
        .map(|[_, y]| *y)
        .fold(0.0, f64::max);
    Series {
        shown,
        game,
        gpu,
        repeated,
        top: (highest * 1.1).max(frame * 1.5).min(frame * 3.0),
    }
}

impl Widget for FrameGraph<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let dark = ui.visuals().dark_mode;
        let shown_color = ui.visuals().strong_text_color();
        let repeated_color = ui.visuals().warn_fg_color;
        let budget_color = ui.visuals().weak_text_color();
        let height = if touch_square(ui) > 0.0 {
            PHONE_HEIGHT
        } else {
            DESKTOP_HEIGHT
        };
        let series = series(self.samples, self.budget.game);
        let budget_name = format!("Frame {}", budget_text(self.budget.game));
        ui.vertical(|ui| {
            Plot::new("performance_frames")
                .height(height)
                .allow_drag(false)
                .allow_zoom(false)
                .allow_scroll(false)
                .allow_boxed_zoom(false)
                .allow_double_click_reset(false)
                .show_x(false)
                .show_y(false)
                .show_axes([false, true])
                .y_axis_label("ms")
                .y_axis_min_width(18.0)
                .y_axis_formatter(|mark, _| format!("{:.0}", mark.value))
                .show(ui, |plot| {
                    plot.set_plot_bounds(PlotBounds::from_min_max(
                        [-SHOWN.as_secs_f64(), 0.0],
                        [0.0, series.top],
                    ));
                    plot.line(
                        Line::new("Shown", PlotPoints::new(series.shown))
                            .color(shown_color)
                            .width(1.0_f32),
                    );
                    plot.hline(
                        HLine::new(budget_name.as_str(), ms(self.budget.game))
                            .color(budget_color)
                            .style(LineStyle::dashed_loose()),
                    );
                    plot.line(
                        Line::new("Game", PlotPoints::new(series.game))
                            .color(pick(GAME, dark))
                            .width(1.0_f32),
                    );
                    if self.gpu {
                        plot.points(
                            Points::new("3D GPU", PlotPoints::new(series.gpu))
                                .color(pick(GPU, dark))
                                .radius(1.5_f32),
                        );
                    }
                    plot.points(
                        Points::new("Repeated", PlotPoints::new(series.repeated))
                            .shape(MarkerShape::Diamond)
                            .color(repeated_color)
                            .radius(2.5_f32),
                    );
                });
            ui.horizontal_wrapped(|ui| {
                key(ui, Swatch::Line, shown_color, "Shown");
                key(ui, Swatch::Line, pick(GAME, dark), "Game");
                if self.gpu {
                    key(ui, Swatch::Dots, pick(GPU, dark), "3D GPU");
                }
                key(ui, Swatch::Diamond, repeated_color, "Repeated");
                key(ui, Swatch::Dashes, budget_color, &budget_name);
            });
        })
        .response
    }
}

#[derive(Clone, Copy)]
enum Swatch {
    Line,
    Dots,
    Diamond,
    Dashes,
}

const SWATCH: egui::Vec2 = vec2(12.0, 10.0);

fn key(ui: &mut Ui, swatch: Swatch, color: Color32, name: &str) -> [egui::Rect; 2] {
    let font = TextStyle::Small.resolve(ui.style());
    let text = ui
        .painter()
        .layout_no_wrap(name.to_string(), font, Color32::PLACEHOLDER)
        .size()
        .x;
    let spacing = ui.spacing().item_spacing.x;
    if SWATCH.x + 2.0 * spacing + text > ui.available_size_before_wrap().x {
        ui.end_row();
    }
    let (rect, _) = ui.allocate_exact_size(SWATCH, Sense::hover());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let center = rect.center();
        let stroke = Stroke::new(1.5_f32, color);
        match swatch {
            Swatch::Line => {
                painter.line_segment([rect.left_center(), rect.right_center()], stroke);
            }
            Swatch::Dots => {
                for step in 0..4 {
                    let at = rect.left_center() + vec2(1.5 + 3.0 * step as f32, 0.0);
                    painter.circle_filled(at, 1.5, color);
                }
            }
            Swatch::Diamond => {
                painter.add(Shape::convex_polygon(
                    vec![
                        center + vec2(0.0, -3.5),
                        center + vec2(3.5, 0.0),
                        center + vec2(0.0, 3.5),
                        center + vec2(-3.5, 0.0),
                    ],
                    color,
                    Stroke::NONE,
                ));
            }
            Swatch::Dashes => {
                for start in [0.0, 5.0, 10.0] {
                    let from = rect.left_center() + vec2(start, 0.0);
                    painter.line_segment([from, from + vec2(2.0, 0.0)], stroke);
                }
            }
        }
    }
    let label = ui.add(Label::new(RichText::new(name).small()).extend());
    [rect, label.rect]
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: Duration = Duration::from_micros(16_700);

    fn sample(
        at_ms: u64,
        display_ms: u64,
        game_frames: u64,
        repeated: u64,
        gpu: Option<u64>,
    ) -> FrameSample {
        FrameSample {
            at: Duration::from_millis(at_ms),
            display: Duration::from_millis(display_ms),
            ui: Duration::ZERO,
            game: Duration::from_millis(7 * game_frames),
            game_frames,
            repeated,
            dropped: 0,
            gpu: gpu.map(Duration::from_millis),
            gpu_frames: u64::from(gpu.is_some()),
            gpu_full: None,
            gpu_passes: None,
            jit_compile: Duration::ZERO,
            jit_blocks: 0,
            console_wait: Duration::ZERO,
            audio: Default::default(),
            budget: Budget {
                game: FRAME,
                refresh: None,
            },
        }
    }

    fn near(point: [f64; 2], want: [f64; 2]) -> bool {
        (point[0] - want[0]).abs() < 1e-9 && (point[1] - want[1]).abs() < 1e-9
    }

    #[test]
    fn the_graph_has_one_point_per_game_frame_and_marks_the_repeated_ones() {
        let samples: VecDeque<_> = [
            sample(1000, 6, 1, 0, Some(5)),
            sample(1006, 6, 0, 0, None),
            sample(1012, 6, 0, 0, None),
            sample(1018, 6, 1, 0, None),
            sample(1024, 6, 0, 0, None),
            sample(1030, 6, 0, 0, None),
            sample(1036, 6, 0, 0, None),
            sample(1042, 6, 1, 1, None),
        ]
        .into();
        let series = series(&samples, FRAME);
        assert_eq!(series.shown.len(), 2, "{:?}", series.shown);
        assert!(near(series.shown[0], [-0.024, 18.0]), "{:?}", series.shown);
        assert!(near(series.shown[1], [0.0, 24.0]), "{:?}", series.shown);
        assert_eq!(series.repeated, [series.shown[1]]);
        assert_eq!(series.game.len(), 3);
        assert!(near(series.game[2], [0.0, 7.0]), "{:?}", series.game);
        assert_eq!(series.gpu.len(), 1);
        assert!(near(series.gpu[0], [-0.042, 5.0]), "{:?}", series.gpu);
        assert!((series.top - 26.4).abs() < 1e-9, "{}", series.top);
    }

    #[test]
    fn one_long_stall_never_flattens_the_rest_of_the_graph() {
        let samples: VecDeque<_> =
            [sample(0, 17, 1, 0, None), sample(900, 900, 1, 50, None)].into();
        let series = series(&samples, FRAME);
        assert!((series.top - 50.1).abs() < 1e-9, "{}", series.top);
    }

    #[test]
    fn short_frames_fill_the_graph_up_to_one_and_a_half_game_frames() {
        let samples: VecDeque<_> = [sample(0, 7, 1, 0, Some(7)), sample(7, 7, 1, 0, None)].into();
        let series = series(&samples, FRAME);
        assert!((series.top - 25.05).abs() < 1e-9, "{}", series.top);
    }

    #[test]
    fn a_gpu_time_above_the_shown_line_stays_in_view() {
        let samples: VecDeque<_> =
            [sample(0, 28, 1, 0, Some(45)), sample(28, 28, 1, 0, None)].into();
        let series = series(&samples, FRAME);
        assert!(near(series.gpu[0], [-0.028, 45.0]), "{:?}", series.gpu);
        assert!((series.top - 49.5).abs() < 1e-9, "{}", series.top);
    }

    #[test]
    fn a_legend_key_wraps_whole_never_between_its_swatch_and_its_name() {
        let ctx = egui::Context::default();
        let mut keys = Vec::new();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                vec2(190.0, 400.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for name in ["Shown", "Game", "3D GPU", "Repeated", "Frame 16.7 ms"] {
                        keys.push(key(ui, Swatch::Dashes, Color32::GRAY, name));
                    }
                });
            });
        });
        let rows: Vec<f32> = keys.iter().map(|[swatch, _]| swatch.top()).collect();
        assert!(rows.iter().any(|top| *top > rows[0]), "{rows:?}");
        for [swatch, label] in keys {
            assert!(
                (swatch.center().y - label.center().y).abs() < 4.0,
                "{swatch:?} {label:?}"
            );
            assert!(label.left() >= swatch.right(), "{swatch:?} {label:?}");
        }
    }
}

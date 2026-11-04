use crate::app::mobile::kit::icon_tile_fill;
use crate::app::ui_state::ui_theme::brand;
use crate::progress::Progress;
use egui::{
    CornerRadius, Label, Rect, Response, RichText, Sense, TextStyle, TextWrapMode, Ui, Widget,
    WidgetText, vec2,
};

pub const BAR_HEIGHT: f32 = 6.0;
const SLIDE_SECONDS: f64 = 1.4;
const SEGMENT: f32 = 0.3;
const LABEL_SHARE: f32 = 0.6;
const WAITING_ALPHA: f32 = 0.35;
const POLL: std::time::Duration = std::time::Duration::from_millis(100);
const LINE_GAP: f32 = 6.0;

pub struct ProgressBar<'a> {
    label: &'a str,
    progress: Progress,
}

impl<'a> ProgressBar<'a> {
    pub fn new(label: &'a str, progress: Progress) -> Self {
        Self { label, progress }
    }

    pub fn tallest(line: f32) -> f32 {
        2.0 * (line + LINE_GAP) + BAR_HEIGHT
    }

    pub fn height(&self, ui: &Ui, width: f32) -> f32 {
        let lines = match self.progress.widest_amount() {
            Some(widest) if !fits_beside(ui, width, self.label, &widest) => 2.0,
            _ => 1.0,
        };
        lines * (ui.text_style_height(&TextStyle::Body) + LINE_GAP) + BAR_HEIGHT
    }
}

impl Widget for ProgressBar<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let amount = self.progress.amount();
        let widest = self.progress.widest_amount().unwrap_or_default();
        let response = ui
            .vertical(|ui| {
                ui.spacing_mut().item_spacing.y = LINE_GAP;
                ui.spacing_mut().interact_size.y = 0.0;
                match &amount {
                    Some(amount) if !fits_beside(ui, ui.available_width(), self.label, &widest) => {
                        ui.horizontal(|ui| ui.add(Label::new(self.label).truncate()));
                        ui.horizontal(|ui| {
                            ui.add(Label::new(RichText::new(amount).weak()).truncate())
                        });
                    }
                    Some(amount) => {
                        ui.horizontal(|ui| {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(RichText::new(amount).weak());
                                    ui.with_layout(
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            ui.add(Label::new(self.label).truncate());
                                        },
                                    );
                                },
                            );
                        });
                    }
                    None => {
                        ui.horizontal(|ui| ui.add(Label::new(self.label).truncate()));
                    }
                }
                bar(ui, self.progress.fraction());
                if self.progress.fraction().is_some_and(|f| f < 1.0) {
                    ui.ctx().request_repaint_after(POLL);
                }
            })
            .response;
        let value = self.progress.fraction().map(f64::from);
        response.widget_info(|| {
            let mut info =
                egui::WidgetInfo::labeled(egui::WidgetType::ProgressIndicator, true, self.label);
            info.value = value;
            info
        });
        response
    }
}

fn fits_beside(ui: &Ui, available: f32, label: &str, amount: &str) -> bool {
    let width = |text: &str| {
        WidgetText::from(text)
            .into_galley(
                ui,
                Some(TextWrapMode::Extend),
                f32::INFINITY,
                TextStyle::Body,
            )
            .size()
            .x
    };
    let room = available - width(amount) - ui.spacing().item_spacing.x;
    room >= width(label).min(available * LABEL_SHARE)
}

pub fn bar(ui: &mut Ui, fraction: Option<f32>) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), BAR_HEIGHT), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let visuals = ui.visuals();
    let radius = CornerRadius::same((BAR_HEIGHT / 2.0) as u8);
    let fill = brand(visuals);
    ui.painter()
        .rect_filled(rect, radius, icon_tile_fill(visuals));
    let (start, end) = match fraction {
        Some(fraction) if fraction > 0.0 => (0.0, (rect.width() * fraction).max(rect.height())),
        Some(_) => return response,
        None if crate::utils::reduced_motion() => {
            ui.painter()
                .rect_filled(rect, radius, fill.gamma_multiply(WAITING_ALPHA));
            return response;
        }
        None => {
            let span = rect.width() * SEGMENT;
            let time = ui.input(|i| i.time);
            ui.ctx().request_repaint();
            let phase = (time % SLIDE_SECONDS / SLIDE_SECONDS) as f32;
            let start = phase * (rect.width() + span) - span;
            (start.max(0.0), (start + span).min(rect.width()))
        }
    };
    if end > start {
        ui.painter().rect_filled(
            Rect::from_min_max(
                rect.min + vec2(start, 0.0),
                rect.min + vec2(end, rect.height()),
            ),
            radius,
            fill,
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Context, Pos2, RawInput, Shape, epaint::ClippedShape};

    struct Frame {
        texts: Vec<String>,
        fills: Vec<Rect>,
        repaint: Option<std::time::Duration>,
        height: f32,
        predicted: f32,
    }

    fn draw(label: &str, progress: Progress) -> Frame {
        draw_at(300.0, label, progress)
    }

    fn draw_at(width: f32, label: &str, progress: Progress) -> Frame {
        let ctx = Context::default();
        let mut height = 0.0;
        let mut predicted = 0.0;
        let mut run = || {
            ctx.run(
                RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(412.0, 200.0))),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.allocate_ui(vec2(width, 200.0), |ui| {
                            let bar = ProgressBar::new(label, progress);
                            predicted = bar.height(ui, ui.available_width());
                            height = ui.add(bar).rect.height();
                        });
                    });
                },
            )
        };
        run();
        let output = run();
        let mut texts = Vec::new();
        let mut fills = Vec::new();
        for ClippedShape { shape, .. } in output.shapes {
            match shape {
                Shape::Text(text) => texts.push(text.galley.text().to_owned()),
                Shape::Rect(rect) if rect.rect.height() == BAR_HEIGHT => fills.push(rect.rect),
                _ => {}
            }
        }
        let repaint = output
            .viewport_output
            .values()
            .map(|v| v.repaint_delay)
            .min()
            .filter(|delay| *delay < std::time::Duration::from_secs(1));
        Frame {
            texts,
            fills,
            repaint,
            height,
            predicted,
        }
    }

    #[test]
    fn the_bar_knows_its_height_before_it_draws() {
        let label = "Getting Lemon Quest from Mia";
        for frame in [
            draw("A", Progress::unknown()),
            draw("A", Progress::bytes(1, 2)),
            draw_at(380.0, label, Progress::bytes(1024 * 1024, 4096 * 1024)),
            draw_at(110.0, label, Progress::bytes(1024 * 1024, 4096 * 1024)),
        ] {
            assert!(
                (frame.height - frame.predicted).abs() < 0.5,
                "{} {}",
                frame.height,
                frame.predicted
            );
        }
    }

    #[test]
    fn a_known_size_fills_the_bar_by_its_share_and_says_how_much() {
        let frame = draw("Getting Snake", Progress::bytes(512 * 1024, 2048 * 1024));
        assert!(frame.texts.contains(&"Getting Snake".to_owned()));
        assert!(
            frame.texts.contains(&"0.5 of 2.0 MB".to_owned()),
            "{:?}",
            frame.texts
        );
        let [track, fill] = frame.fills[..] else {
            panic!("want a track and a fill, got {:?}", frame.fills);
        };
        assert!((fill.width() - track.width() / 4.0).abs() < 1.0);
        assert!(
            frame
                .repaint
                .is_some_and(|delay| delay > std::time::Duration::ZERO && delay <= POLL),
            "{:?}",
            frame.repaint
        );
    }

    #[test]
    fn an_unknown_size_moves_and_says_no_amount() {
        let frame = draw("Reading the folder", Progress::unknown());
        assert_eq!(frame.texts, ["Reading the folder"]);
        assert_eq!(frame.repaint, Some(std::time::Duration::ZERO));
        assert!(frame.fills.len() <= 2);
    }

    #[test]
    fn done_fills_the_whole_track() {
        let frame = draw("Saving", Progress::items(5, 5, "files"));
        assert!(frame.texts.contains(&"5 of 5 files".to_owned()));
        let [track, fill] = frame.fills[..] else {
            panic!("want a track and a fill, got {:?}", frame.fills);
        };
        assert_eq!(fill, track);
        assert_eq!(frame.repaint, None);
    }

    #[test]
    fn every_state_is_one_line_over_the_bar() {
        let heights = [
            draw("A", Progress::unknown()).height,
            draw("A", Progress::bytes(1, 2)).height,
            draw(&"A very long game title ".repeat(8), Progress::bytes(1, 2)).height,
        ];
        assert!(
            heights.iter().all(|h| (*h - heights[0]).abs() < 0.5),
            "{heights:?}"
        );
        assert!(heights[0] < 2.0 * 18.0 + BAR_HEIGHT, "{heights:?}");
    }

    #[test]
    fn a_narrow_bar_keeps_its_label_and_puts_the_amount_below() {
        let label = "Getting Lemon Quest from Mia";
        let wide = draw_at(380.0, label, Progress::bytes(1024 * 1024, 4096 * 1024));
        let narrow = draw_at(110.0, label, Progress::bytes(1024 * 1024, 4096 * 1024));
        assert!(wide.texts.contains(&label.to_owned()), "{:?}", wide.texts);
        assert!(
            narrow.texts.contains(&"1.0 of 4.0 MB".to_owned()),
            "{:?}",
            narrow.texts
        );
        assert!(
            narrow.texts.iter().any(|t| t.starts_with("Getting Lemon")),
            "{:?}",
            narrow.texts
        );
        assert!(
            narrow.height > wide.height,
            "{} {}",
            narrow.height,
            wide.height
        );
    }
}

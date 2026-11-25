use crate::app::widgets::frame_graph::{GAME, GPU, pick};
use crate::frame_log::TapeTimes;
use crate::media::playback::clock;
use egui::{Id, Rangef, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};

const DESKTOP_HEIGHT: f32 = 44.0;
const PHONE_HEIGHT: f32 = 56.0;

const READOUT_PAD: Vec2 = Vec2::new(6.0, 2.0);

pub fn height(phone: bool) -> f32 {
    if phone { PHONE_HEIGHT } else { DESKTOP_HEIGHT }
}

fn band(ui: &Ui) -> f32 {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let row = ui
        .painter()
        .layout_no_wrap("0".to_owned(), font, ui.visuals().text_color());
    row.size().y + 2.0 * READOUT_PAD.y
}

pub fn widget_height(ui: &Ui, phone: bool) -> f32 {
    band(ui) + height(phone)
}

fn plot_rect(rect: Rect, phone: bool) -> Rect {
    rect.with_min_y(rect.bottom() - height(phone))
}

pub fn seek_span_id() -> Id {
    Id::new("replay_seek_span")
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timeline {
    pub span: Rangef,
    pub frames: u64,
}

impl Timeline {
    pub fn x(&self, frame: u64) -> f32 {
        if self.frames <= 1 {
            return self.span.min;
        }
        let t = (frame.clamp(1, self.frames) - 1) as f32 / (self.frames - 1) as f32;
        self.span.min + t * self.span.span()
    }

    pub fn frame(&self, x: f32) -> u64 {
        if self.frames <= 1 || self.span.span() <= 0.0 {
            return 1;
        }
        let t = ((x - self.span.min) / self.span.span()).clamp(0.0, 1.0);
        1 + (t * (self.frames - 1) as f32).round() as u64
    }
}

pub fn hover_text(times: &TapeTimes, frame: u64, frame_secs: f64) -> String {
    let at = clock(frame as f64 * frame_secs);
    let game = times.game_at(frame).unwrap_or_default();
    let of = if times.budget_ms > 0.0 {
        format!(" of {:.1} ms", times.budget_ms)
    } else {
        String::new()
    };
    match times.gpu_at(frame) {
        Some(gpu) => format!("Frame at {at} took {game:.1} ms{of}, 3D GPU {gpu:.1} ms"),
        None => format!("Frame at {at} took {game:.1} ms{of}"),
    }
}

fn readout_rect(graph: Rect, x: f32, size: Vec2) -> Rect {
    let left = (x - size.x / 2.0).clamp(graph.left(), (graph.right() - size.x).max(graph.left()));
    Rect::from_min_size(pos2(left, graph.top()), size)
}

fn readout(ui: &Ui, graph: Rect, plot: Rect, x: f32, text: String) {
    let visuals = ui.visuals();
    let painter = ui.painter_at(graph);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let galley = painter.layout(
        text,
        font,
        visuals.text_color(),
        graph.width() - 2.0 * READOUT_PAD.x,
    );
    let label = readout_rect(graph, x, galley.size() + 2.0 * READOUT_PAD);
    painter.line_segment(
        [pos2(x, label.bottom()), pos2(x, plot.bottom())],
        Stroke::new(1.0_f32, visuals.weak_text_color()),
    );
    painter.rect(
        label,
        visuals.menu_corner_radius,
        visuals.window_fill,
        visuals.window_stroke,
        StrokeKind::Inside,
    );
    painter.galley(label.min + READOUT_PAD, galley, visuals.text_color());
}

fn dashes(length: f32) -> (f32, f32) {
    let count = ((length + 4.0) / 8.0).floor();
    if count < 2.0 {
        return (length.max(0.0), 0.0);
    }
    (4.0, (length - count * 4.0) / (count - 1.0))
}

fn columns(series: &[(u64, f32)], line: &Timeline) -> Vec<(i32, f32)> {
    let mut out: Vec<(i32, f32)> = Vec::new();
    for &(frame, ms) in series {
        let x = line.x(frame).round() as i32;
        match out.last_mut() {
            Some((last, top)) if *last == x => *top = top.max(ms),
            _ => out.push((x, ms)),
        }
    }
    out
}

pub fn replay_graph(
    ui: &mut Ui,
    times: &TapeTimes,
    frames: u64,
    at: u64,
    frame_secs: f64,
    phone: bool,
) -> Option<u64> {
    let size = vec2(ui.available_width(), widget_height(ui, phone));
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let plot = plot_rect(rect, phone);
    let span = ui
        .data(|d| d.get_temp::<Rangef>(seek_span_id()))
        .filter(|span| span.span() > 0.0)
        .unwrap_or(rect.x_range());
    let response = ui.interact(
        Rect::from_x_y_ranges(span, plot.y_range()),
        ui.id().with("replay_graph"),
        Sense::click_and_drag(),
    );
    let line = Timeline { span, frames };
    if ui.is_rect_visible(rect) {
        let visuals = ui.visuals();
        let dark = visuals.dark_mode;
        let top = times.top();
        let y = |ms: f32| plot.bottom() - (ms / top).clamp(0.0, 1.0) * plot.height();
        let painter = ui.painter_at(rect);
        if times.budget_ms > 0.0 {
            let budget = y(times.budget_ms);
            let (dash, gap) = dashes(span.span());
            painter.extend(Shape::dashed_line(
                &[pos2(span.min, budget), pos2(span.max, budget)],
                Stroke::new(1.0_f32, visuals.weak_text_color()),
                dash,
                gap,
            ));
        }
        let game = columns(&times.game, &line)
            .into_iter()
            .map(|(x, ms)| pos2(x as f32, y(ms)))
            .collect();
        painter.add(Shape::line(game, Stroke::new(1.0_f32, pick(GAME, dark))));
        for (x, ms) in columns(&times.gpu, &line) {
            painter.circle_filled(pos2(x as f32, y(ms)), 1.5, pick(GPU, dark));
        }
        let marker = line.x(at);
        painter.line_segment(
            [pos2(marker, plot.top()), pos2(marker, plot.bottom())],
            Stroke::new(1.5_f32, visuals.strong_text_color()),
        );
    }
    let seek = if response.clicked() || response.dragged() {
        response
            .interact_pointer_pos()
            .map(|pos| line.frame(pos.x))
            .filter(|to| *to != at)
    } else {
        None
    };
    if let Some(pos) = response.hover_pos() {
        let frame = line.frame(pos.x);
        readout(
            ui,
            rect,
            plot,
            line.x(frame),
            hover_text(times, frame, frame_secs),
        );
    }
    seek
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_budget_line_ends_on_a_dash_at_both_ends() {
        for length in [0.5, 7.0, 543.2, 816.7] {
            let (dash, gap) = dashes(length);
            let count = ((length + gap) / (dash + gap)).round();
            assert!(
                (count * dash + (count - 1.0) * gap - length).abs() < 0.01,
                "{length}"
            );
        }
    }

    #[test]
    fn the_graph_maps_frames_to_the_same_places_as_the_slider() {
        let line = Timeline {
            span: Rangef::new(100.0, 300.0),
            frames: 201,
        };
        assert_eq!(line.x(1), 100.0);
        assert_eq!(line.x(101), 200.0);
        assert_eq!(line.x(201), 300.0);
        assert_eq!(line.x(900), 300.0);
        assert_eq!(line.frame(200.0), 101);
        assert_eq!(line.frame(50.0), 1);
        assert_eq!(line.frame(350.0), 201);
        for frame in [1, 7, 150, 201] {
            assert_eq!(line.frame(line.x(frame)), frame);
        }
    }

    #[test]
    fn many_frames_on_one_pixel_keep_the_slowest() {
        let line = Timeline {
            span: Rangef::new(0.0, 10.0),
            frames: 1001,
        };
        let series = [(1, 3.0), (20, 9.0), (40, 5.0), (1001, 2.0)];
        assert_eq!(columns(&series, &line), vec![(0, 9.0), (10, 2.0)]);
    }

    #[test]
    fn hover_names_the_time_and_the_frame_time() {
        let times = TapeTimes {
            game: vec![(60, 41.2)],
            gpu: Vec::new(),
            budget_ms: 16.7,
        };
        assert_eq!(
            hover_text(&times, 60, 1.0),
            "Frame at 1:00 took 41.2 ms of 16.7 ms"
        );
        let times = TapeTimes {
            gpu: vec![(60, 12.0)],
            ..times
        };
        assert_eq!(
            hover_text(&times, 60, 1.0),
            "Frame at 1:00 took 41.2 ms of 16.7 ms, 3D GPU 12.0 ms"
        );
        let times = TapeTimes {
            budget_ms: 0.0,
            ..times
        };
        assert_eq!(
            hover_text(&times, 60, 1.0),
            "Frame at 1:00 took 41.2 ms, 3D GPU 12.0 ms"
        );
    }

    fn click_graph(x: f32) -> Option<u64> {
        let times = TapeTimes {
            game: vec![(1, 2.0), (201, 3.0)],
            gpu: Vec::new(),
            budget_ms: 16.7,
        };
        let ctx = egui::Context::default();
        ctx.data_mut(|d| d.insert_temp(seek_span_id(), Rangef::new(100.0, 300.0)));
        let mut seek = None;
        let mut y = 0.0;
        for pass in 0..3 {
            let pos = pos2(x, y);
            let press = |pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            };
            let events = match pass {
                1 => vec![egui::Event::PointerMoved(pos), press(true)],
                2 => vec![press(false)],
                _ => Vec::new(),
            };
            let raw = egui::RawInput {
                events,
                ..Default::default()
            };
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    y = ui.cursor().top() + widget_height(ui, false) - 10.0;
                    seek = replay_graph(ui, &times, 201, 150, 1.0 / 60.0, false).or(seek);
                });
            });
        }
        seek
    }

    #[test]
    fn only_the_drawn_timeline_takes_a_click() {
        assert_eq!(click_graph(50.0), None);
        assert_eq!(click_graph(350.0), None);
        let seek = click_graph(200.0).expect("a click on the line seeks");
        assert!(seek.abs_diff(101) <= 1, "{seek}");
    }

    #[test]
    fn the_readout_sits_above_the_plot() {
        let times = TapeTimes {
            game: vec![(60, 41.2)],
            gpu: vec![(60, 12.0)],
            budget_ms: 16.7,
        };
        let mut checked = 0;
        let _ = egui::Context::default().run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let font = egui::TextStyle::Body.resolve(ui.style());
                let text = hover_text(&times, 60, 1.0);
                let galley = ui
                    .painter()
                    .layout_no_wrap(text, font, egui::Color32::WHITE);
                let size = galley.size() + 2.0 * READOUT_PAD;
                for phone in [false, true] {
                    let graph = Rect::from_min_size(
                        pos2(10.0, 50.0),
                        vec2(400.0, widget_height(ui, phone)),
                    );
                    let plot = plot_rect(graph, phone);
                    assert_eq!(plot.height(), height(phone));
                    for x in [0.0, 10.0, 210.0, 409.0, 500.0] {
                        let label = readout_rect(graph, x, size);
                        assert!(graph.contains_rect(label), "{label:?}");
                        assert!(label.bottom() <= plot.top(), "{label:?} {plot:?}");
                        checked += 1;
                    }
                }
            });
        });
        assert_eq!(checked, 10);
    }

    #[test]
    fn the_hover_readout_stays_inside_the_graph() {
        let graph = Rect::from_min_size(pos2(10.0, 50.0), vec2(200.0, 44.0));
        let size = vec2(80.0, 18.0);
        let middle = readout_rect(graph, 110.0, size);
        assert_eq!(middle.center().x, 110.0);
        assert_eq!(middle.top(), graph.top());
        assert_eq!(readout_rect(graph, 12.0, size).left(), graph.left());
        assert_eq!(readout_rect(graph, 209.0, size).right(), graph.right());
        for x in [0.0, 10.0, 60.0, 180.0, 400.0] {
            assert!(graph.contains_rect(readout_rect(graph, x, size)), "{x}");
        }
    }
}

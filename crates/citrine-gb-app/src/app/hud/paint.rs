use super::cells::{Facts, cell};
use super::{Direction, Group, Hud, INSET, Value, shown_values};
use egui::text::LayoutJob;
use egui::{
    Align, Color32, FontId, Galley, Painter, Pos2, Rect, Stroke, TextFormat, Vec2, pos2, vec2,
};
use std::sync::Arc;

const LABEL: f32 = 11.0;
const NUMBER: f32 = 13.0;
const ROW_GRAPH: Vec2 = vec2(66.0, 16.0);
const COLUMN_GRAPH: Vec2 = vec2(128.0, 22.0);
const GRAPH_TOP: f32 = 40.0;
const OUTLINE: [Vec2; 8] = [
    vec2(-1.0, -1.0),
    vec2(0.0, -1.0),
    vec2(1.0, -1.0),
    vec2(-1.0, 0.0),
    vec2(1.0, 0.0),
    vec2(-1.0, 1.0),
    vec2(0.0, 1.0),
    vec2(1.0, 1.0),
];

#[derive(Debug, Clone, Copy)]
pub struct Look {
    pub dark: bool,
    pub opacity: f32,
}

impl Look {
    pub fn of(visuals: &egui::Visuals, opacity: f32) -> Look {
        Look {
            dark: visuals.dark_mode,
            opacity: opacity.clamp(0.0, 1.0),
        }
    }

    pub fn text(self) -> Color32 {
        if self.dark {
            Color32::from_gray(242)
        } else {
            Color32::from_gray(24)
        }
    }

    fn edge(self) -> Color32 {
        if self.dark {
            Color32::BLACK
        } else {
            Color32::WHITE
        }
    }

    fn card(self) -> Color32 {
        let alpha = (self.opacity * 255.0).round() as u8;
        if self.dark {
            Color32::from_rgba_unmultiplied(27, 27, 27, alpha)
        } else {
            Color32::from_rgba_unmultiplied(250, 250, 250, alpha)
        }
    }

    fn line(self) -> Color32 {
        let alpha = (self.opacity * 40.0).round() as u8;
        if self.dark {
            Color32::from_white_alpha(alpha)
        } else {
            Color32::from_black_alpha(alpha)
        }
    }

    pub fn group(self, group: Group) -> Color32 {
        let (dark, light) = match group {
            Group::Rate => ((245, 173, 51), (138, 94, 0)),
            Group::Smoothness => ((76, 194, 122), (23, 115, 61)),
            Group::Load => ((110, 168, 255), (31, 85, 173)),
            Group::Sound => ((190, 150, 255), (116, 52, 176)),
            Group::Game => ((255, 145, 100), (168, 58, 18)),
            Group::Device => ((190, 190, 190), (80, 80, 80)),
            Group::Online => ((64, 206, 214), (0, 116, 128)),
        };
        let (r, g, b) = if self.dark { dark } else { light };
        Color32::from_rgb(r, g, b)
    }

    fn late(self) -> Color32 {
        if self.dark {
            Color32::from_rgb(255, 107, 94)
        } else {
            Color32::from_rgb(208, 52, 44)
        }
    }
}

enum Piece {
    Text { galley: Arc<Galley>, at: Vec2 },
    Graph(Rect),
    Line { from: Vec2, to: Vec2 },
}

pub struct Laid {
    pub size: Vec2,
    radius: f32,
    scale: f32,
    pieces: Vec<Piece>,
}

fn format(size: f32, color: Color32) -> TextFormat {
    TextFormat {
        font_id: FontId::proportional(size),
        color,
        valign: Align::BOTTOM,
        ..Default::default()
    }
}

fn number_job(job: &mut LayoutJob, value: &str, unit: &str, scale: f32, look: Look, leading: f32) {
    job.append(value, leading, format(NUMBER * scale, look.text()));
    if !unit.is_empty() {
        let gap = if unit == "%" { 0.5 } else { 3.0 } * scale;
        job.append(unit, gap, format(LABEL * scale, look.text()));
    }
}

pub fn lay(painter: &Painter, hud: &Hud, facts: &Facts, look: Look, room: Vec2) -> Laid {
    let values: Vec<Value> = shown_values(&hud.values, facts.ds).collect();
    let mut scale = hud.scale();
    let mut laid = lay_at(painter, hud.direction, &values, facts, look, scale, room);
    for _ in 0..4 {
        let fit = (room.x / laid.size.x).min(room.y / laid.size.y);
        if fit >= 1.0 || !fit.is_finite() {
            break;
        }
        scale *= fit * 0.98;
        laid = lay_at(painter, hud.direction, &values, facts, look, scale, room);
    }
    laid
}

const TITLE_WIDTH: f32 = 170.0;

pub fn lay_lines(
    painter: &Painter,
    hud: &Hud,
    lines: &[(String, String)],
    look: Look,
    room: Vec2,
) -> Laid {
    let mut scale = hud.scale();
    let mut laid = lay_lines_at(painter, lines, look, scale);
    for _ in 0..4 {
        let fit = (room.x / laid.size.x).min(room.y / laid.size.y);
        if fit >= 1.0 || !fit.is_finite() {
            break;
        }
        scale *= fit * 0.98;
        laid = lay_lines_at(painter, lines, look, scale);
    }
    laid
}

fn lay_lines_at(painter: &Painter, lines: &[(String, String)], look: Look, scale: f32) -> Laid {
    let pad = vec2(9.0, 6.0) * scale;
    let gap = 8.0 * scale;
    let row_gap = 4.0 * scale;
    let rows: Vec<(Arc<Galley>, Arc<Galley>)> = lines
        .iter()
        .map(|(title, score)| {
            let mut job = LayoutJob::single_section(
                title.clone(),
                format(LABEL * scale, look.group(Group::Game)),
            );
            job.wrap.max_width = TITLE_WIDTH * scale;
            job.wrap.max_rows = 1;
            job.wrap.break_anywhere = true;
            let title = painter.layout_job(job);
            let mut job = LayoutJob::default();
            number_job(&mut job, score, "", scale, look, 0.0);
            (title, painter.layout_job(job))
        })
        .collect();
    let title_width = rows
        .iter()
        .map(|(title, _)| title.size().x)
        .fold(0.0, f32::max);
    let score_width = rows
        .iter()
        .map(|(_, score)| score.size().x)
        .fold(0.0, f32::max);
    let mut pieces = Vec::with_capacity(rows.len() * 2);
    let mut y = pad.y;
    for (title, score) in rows {
        let height = title.size().y.max(score.size().y);
        let score_x = pad.x + title_width + gap + score_width - score.size().x;
        pieces.push(Piece::Text {
            at: vec2(pad.x, y + (height - title.size().y) / 2.0),
            galley: title,
        });
        pieces.push(Piece::Text {
            at: vec2(score_x, y + (height - score.size().y) / 2.0),
            galley: score,
        });
        y += height + row_gap;
    }
    Laid {
        size: vec2(
            pad.x * 2.0 + title_width + gap + score_width,
            y - row_gap + pad.y,
        ),
        radius: 8.0 * scale,
        scale,
        pieces,
    }
}

fn lay_at(
    painter: &Painter,
    direction: Direction,
    values: &[Value],
    facts: &Facts,
    look: Look,
    scale: f32,
    room: Vec2,
) -> Laid {
    if values.is_empty() {
        let galley = painter.layout_job(LayoutJob::single_section(
            "No values".into(),
            format(LABEL * scale, look.text()),
        ));
        let pad = vec2(9.0, 6.0) * scale;
        return Laid {
            size: galley.size() + pad * 2.0,
            radius: 8.0 * scale,
            scale,
            pieces: vec![Piece::Text { galley, at: pad }],
        };
    }
    let wrap = room - Vec2::splat(2.0 * INSET);
    match direction {
        Direction::Row => lay_row(painter, values, facts, look, scale, wrap.x),
        Direction::Column => lay_column(painter, values, facts, look, scale, wrap.y),
    }
}

fn lay_row(
    painter: &Painter,
    values: &[Value],
    facts: &Facts,
    look: Look,
    scale: f32,
    wrap: f32,
) -> Laid {
    let pad = vec2(9.0, 6.0) * scale;
    let gap = 11.0 * scale;
    let line_gap = 6.0 * scale;
    let mut items: Vec<(Option<Arc<Galley>>, Vec2)> = Vec::with_capacity(values.len());
    for &value in values {
        if value == Value::Graph {
            items.push((None, ROW_GRAPH * scale));
            continue;
        }
        let cell = cell(value, facts);
        let mut job = LayoutJob::default();
        job.append(
            cell.label,
            0.0,
            format(LABEL * scale, look.group(value.group())),
        );
        number_job(&mut job, &cell.value, &cell.unit, scale, look, 4.0 * scale);
        let galley = painter.layout_job(job);
        let size = galley.size();
        items.push((Some(galley), size));
    }
    let height = items.iter().map(|(_, size)| size.y).fold(0.0, f32::max);
    let mut pieces = Vec::with_capacity(items.len() * 2);
    let mut x = pad.x;
    let mut y = pad.y;
    let mut width: f32 = 0.0;
    for (i, (galley, size)) in items.into_iter().enumerate() {
        if i > 0 && x + size.x + pad.x > wrap {
            width = width.max(x - gap + pad.x);
            x = pad.x;
            y += height + line_gap;
        } else if i > 0 {
            let mid = x - gap / 2.0;
            pieces.push(Piece::Line {
                from: vec2(mid, y),
                to: vec2(mid, y + height),
            });
        }
        let at = vec2(x, y + (height - size.y) / 2.0);
        pieces.push(match galley {
            Some(galley) => Piece::Text { galley, at },
            None => Piece::Graph(Rect::from_min_size(at.to_pos2(), size)),
        });
        x += size.x + gap;
    }
    Laid {
        size: vec2(width.max(x - gap + pad.x), y + height + pad.y),
        radius: 8.0 * scale,
        scale,
        pieces,
    }
}

fn lay_column(
    painter: &Painter,
    values: &[Value],
    facts: &Facts,
    look: Look,
    scale: f32,
    wrap: f32,
) -> Laid {
    let pad = vec2(10.0, 8.0) * scale;
    let row_gap = 6.0 * scale;
    let column_gap = 12.0 * scale;
    let mut rows: Vec<Option<(Arc<Galley>, Arc<Galley>)>> = Vec::with_capacity(values.len());
    for &value in values {
        if value == Value::Graph {
            rows.push(None);
            continue;
        }
        let cell = cell(value, facts);
        let label = painter.layout_job(LayoutJob::single_section(
            cell.label.into(),
            format(LABEL * scale, look.group(value.group())),
        ));
        let mut job = LayoutJob::default();
        number_job(&mut job, &cell.value, &cell.unit, scale, look, 0.0);
        rows.push(Some((label, painter.layout_job(job))));
    }
    let labels = rows
        .iter()
        .flatten()
        .map(|(label, _)| label.size().x)
        .fold(0.0, f32::max);
    let numbers = rows
        .iter()
        .flatten()
        .map(|(_, number)| number.size().x)
        .fold(0.0, f32::max);
    let graph = COLUMN_GRAPH * scale;
    let width = (labels + column_gap + numbers).max(if rows.iter().any(Option::is_none) {
        graph.x
    } else {
        0.0
    });
    let between = 2.0 * column_gap;
    let mut pieces = Vec::with_capacity(rows.len() * 2);
    let mut x = pad.x;
    let mut y = pad.y;
    let mut bottom: f32 = 0.0;
    for (i, row) in rows.into_iter().enumerate() {
        let height = match &row {
            Some((label, number)) => label.size().y.max(number.size().y),
            None => graph.y,
        };
        if i > 0 && y + height + pad.y > wrap {
            bottom = bottom.max(y - row_gap + pad.y);
            x += width + between;
            y = pad.y;
            pieces.push(Piece::Line {
                from: vec2(x - between / 2.0, pad.y),
                to: vec2(x - between / 2.0, bottom - pad.y),
            });
        }
        match row {
            Some((label, number)) => {
                pieces.push(Piece::Text {
                    at: vec2(x, y + height - label.size().y),
                    galley: label,
                });
                pieces.push(Piece::Text {
                    at: vec2(x + width - number.size().x, y + height - number.size().y),
                    galley: number,
                });
            }
            None => {
                pieces.push(Piece::Graph(Rect::from_min_size(
                    pos2(x, y),
                    vec2(width, graph.y),
                )));
            }
        }
        y += height + row_gap;
    }
    Laid {
        size: vec2(x + width + pad.x, bottom.max(y - row_gap + pad.y)),
        radius: 8.0 * scale,
        scale,
        pieces,
    }
}

pub fn paint(painter: &Painter, at: Pos2, laid: &Laid, facts: &Facts, look: Look) {
    let rect = Rect::from_min_size(at, laid.size);
    if look.opacity > 0.0 {
        painter.rect(
            rect,
            laid.radius,
            look.card(),
            Stroke::new(1.0_f32, look.line()),
            egui::StrokeKind::Inside,
        );
    }
    let origin = at.to_vec2();
    let edge = look.edge();
    for piece in &laid.pieces {
        match piece {
            Piece::Text { galley, at } => {
                let pos = (*at + origin).to_pos2();
                for ring in outline_rings(laid.scale) {
                    for offset in OUTLINE {
                        painter.galley_with_override_text_color(
                            pos + offset * ring,
                            galley.clone(),
                            edge,
                        );
                    }
                }
                painter.galley(pos, galley.clone(), look.text());
            }
            Piece::Line { from, to } => {
                painter.line_segment(
                    [(*from + origin).to_pos2(), (*to + origin).to_pos2()],
                    Stroke::new(1.0_f32, look.line()),
                );
            }
            Piece::Graph(graph) => graph_paint(painter, graph.translate(origin), facts, look),
        }
    }
}

pub fn outline_rings(scale: f32) -> impl Iterator<Item = f32> {
    let widest = scale.max(1.0);
    let steps = widest.ceil() as usize;
    (1..=steps).map(move |step| widest * step as f32 / steps as f32)
}

fn graph_paint(painter: &Painter, rect: Rect, facts: &Facts, look: Look) {
    let budget = facts
        .readout
        .as_ref()
        .filter(|r| r.rate > 0.0)
        .map_or(1000.0 / 60.0, |r| 1000.0 / r.rate);
    let top = facts
        .strip
        .iter()
        .map(|frame| frame.shown.as_secs_f64() * 1000.0)
        .fold(GRAPH_TOP.into(), f64::max) as f32;
    let y = |ms: f32| rect.bottom() - (ms / top).min(1.0) * rect.height();
    let base = y(budget as f32);
    painter.line_segment(
        [pos2(rect.left(), base), pos2(rect.right(), base)],
        Stroke::new(1.0_f32, look.edge().gamma_multiply(0.6)),
    );
    let count = crate::frame_log::readout::STRIP as f32;
    let step = rect.width() / count;
    let skip = crate::frame_log::readout::STRIP.saturating_sub(facts.strip.len());
    let mut points = Vec::with_capacity(facts.strip.len());
    for (i, frame) in facts.strip.iter().enumerate() {
        let ms = (frame.shown.as_secs_f64() * 1000.0) as f32;
        let x = rect.left() + (skip + i) as f32 * step + step / 2.0;
        let y = y(ms);
        if frame.repeated {
            painter.rect_filled(
                Rect::from_min_max(
                    pos2(x - step.max(2.0) / 2.0, y),
                    pos2(x + step.max(2.0) / 2.0, rect.bottom()),
                ),
                0.0,
                look.late(),
            );
        }
        points.push(pos2(x, y));
    }
    if points.len() > 1 {
        painter.add(egui::Shape::line(
            points.clone(),
            Stroke::new(3.0_f32, look.edge()),
        ));
        painter.add(egui::Shape::line(
            points,
            Stroke::new(1.4_f32, look.group(Group::Smoothness)),
        ));
    }
}

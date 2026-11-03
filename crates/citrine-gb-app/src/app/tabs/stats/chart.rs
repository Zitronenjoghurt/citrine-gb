use super::{CHART_HEIGHT, Range, TOUCH_CELL, WIDE, play_time, scale_top, short_time};
use crate::stats::{day_key, parse_day};
use egui::{Color32, CornerRadius, Rect, Sense, Ui, pos2, vec2};
use jiff::civil::Date;
use std::collections::BTreeMap;

pub(super) const ACCENT: Color32 = Color32::from_rgb(30, 136, 200);

pub(super) fn toward_panel(ui: &Ui, color: Color32, t: f32) -> Color32 {
    color.lerp_to_gamma(ui.visuals().panel_fill, t)
}

pub(super) fn empty_fill(ui: &Ui) -> Color32 {
    toward_panel(ui, ui.visuals().text_color(), 0.88)
}

pub(super) struct Bar {
    pub(super) label: String,
    tip: String,
    ms: u64,
    future: bool,
}

pub(super) fn day_bar(days: &BTreeMap<String, u64>, day: Date, label: String, today: Date) -> Bar {
    let ms = days.get(&day_key(day)).copied().unwrap_or(0);
    Bar {
        label,
        tip: format!("{}: {}", day.strftime("%a %-d %b"), play_time(ms)),
        ms,
        future: day > today,
    }
}

pub(super) fn day_bars(days: &BTreeMap<String, u64>, today: Date, range: Range) -> Vec<Bar> {
    match range {
        Range::Week => {
            let monday = crate::stats::week_start(today);
            (0..7)
                .filter_map(|n| monday.checked_add(jiff::Span::new().days(n)).ok())
                .map(|day| day_bar(days, day, day.strftime("%a").to_string(), today))
                .collect()
        }
        Range::Month => (1..=today.days_in_month())
            .filter_map(|n| today.with().day(n).build().ok())
            .map(|day| day_bar(days, day, day.day().to_string(), today))
            .collect(),
        Range::Year | Range::Calendar => (1..=12)
            .filter_map(|n| Date::new(today.year(), n, 1).ok())
            .map(|month| {
                let ms = days
                    .iter()
                    .filter_map(|(key, ms)| parse_day(key).map(|d| (d, *ms)))
                    .filter(|(d, _)| d.year() == month.year() && d.month() == month.month())
                    .map(|(_, ms)| ms)
                    .sum();
                Bar {
                    label: month.strftime("%b").to_string().chars().take(1).collect(),
                    tip: format!("{}: {}", month.strftime("%B"), play_time(ms)),
                    ms,
                    future: month.month() > today.month(),
                }
            })
            .collect(),
    }
}

pub(super) fn readout(ui: &mut Ui, text: String) {
    let height = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), height),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.add(egui::Label::new(text).truncate());
        },
    );
}

pub(super) fn bars(ui: &mut Ui, id: &str, days: &BTreeMap<String, u64>, today: Date, range: Range) {
    let bars = day_bars(days, today, range);
    let max = scale_top(bars.iter().map(|b| b.ms).max().unwrap_or(0));
    let picked_id = ui.id().with((id, "picked", range.label()));
    let mut picked: Option<usize> = ui.data(|d| d.get_temp(picked_id)).flatten();
    let label_height = 16.0;
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), CHART_HEIGHT + label_height),
        Sense::hover(),
    );
    let plot = Rect::from_min_max(
        rect.min + vec2(0.0, 22.0),
        pos2(rect.max.x, rect.max.y - label_height),
    );
    let painter = ui.painter_at(rect);
    let grid = ui.visuals().widgets.noninteractive.bg_stroke;
    painter.line_segment([plot.left_top(), plot.right_top()], grid);
    painter.line_segment([plot.left_bottom(), plot.right_bottom()], grid);
    painter.text(
        plot.left_top() - vec2(0.0, 2.0),
        egui::Align2::LEFT_BOTTOM,
        short_time(max),
        egui::FontId::proportional(11.0),
        ui.visuals().weak_text_color(),
    );
    let slot = plot.width() / bars.len() as f32;
    let gap = (slot * 0.25).clamp(2.0, 8.0);
    let color = ACCENT;
    let mut hovered = None;
    for (i, bar) in bars.iter().enumerate() {
        let x = plot.left() + slot * i as f32;
        let height = (bar.ms as f32 / max as f32) * plot.height();
        let lit = if bar.future {
            false
        } else {
            let column = Rect::from_min_max(pos2(x, plot.top()), pos2(x + slot, rect.bottom()));
            let response = ui.interact(column, ui.id().with((id, i)), Sense::click());
            if response.clicked() {
                picked = if picked == Some(i) { None } else { Some(i) };
            }
            if response.hovered() {
                hovered = Some(i);
            }
            response.hovered() || picked == Some(i)
        };
        if bar.ms > 0 {
            let shape = Rect::from_min_max(
                pos2(x + gap / 2.0, plot.bottom() - height.max(2.0)),
                pos2(x + slot - gap / 2.0, plot.bottom()),
            );
            let fill = if lit {
                color.lerp_to_gamma(ui.visuals().strong_text_color(), 0.25)
            } else {
                color
            };
            painter.rect_filled(
                shape,
                CornerRadius {
                    nw: 3,
                    ne: 3,
                    sw: 0,
                    se: 0,
                },
                fill,
            );
        }
        let labelled = range != Range::Month || i == 0 || (i + 1) % 5 == 0;
        if labelled || lit {
            painter.text(
                pos2(x + slot / 2.0, rect.bottom()),
                egui::Align2::CENTER_BOTTOM,
                &bar.label,
                egui::FontId::proportional(11.0),
                if lit {
                    ui.visuals().strong_text_color()
                } else {
                    ui.visuals().weak_text_color()
                },
            );
        }
    }
    ui.data_mut(|d| d.insert_temp(picked_id, picked));
    let text = match hovered.or(picked) {
        Some(i) => bars[i].tip.clone(),
        None => format!(
            "{}: {}",
            range.total_label(),
            play_time(bars.iter().map(|b| b.ms).sum())
        ),
    };
    readout(ui, text);
}

pub(super) const LEVELS: [u64; 3] = [30 * 60_000, 60 * 60_000, 120 * 60_000];

pub(super) fn level_fill(ui: &Ui, ms: u64) -> Color32 {
    if ms == 0 {
        return empty_fill(ui);
    }
    let level = LEVELS.iter().filter(|&&edge| ms >= edge).count();
    let steps = if ui.visuals().dark_mode {
        [0.45, 0.3, 0.15, 0.0]
    } else {
        [0.6, 0.4, 0.2, 0.0]
    };
    toward_panel(ui, ACCENT, steps[level])
}

pub(super) fn level_cell(ui: &Ui, painter: &egui::Painter, square: Rect, ms: u64, lit: bool) {
    painter.rect_filled(square, CornerRadius::same(2), level_fill(ui, ms));
    if !lit {
        return;
    }
    let stroke = egui::Stroke::new(1.5_f32, ui.visuals().strong_text_color());
    painter.rect_stroke(
        square,
        CornerRadius::same(2),
        stroke,
        egui::StrokeKind::Inside,
    );
}

pub(super) fn calendar(
    ui: &mut Ui,
    id: &str,
    days: &BTreeMap<String, u64>,
    today: Date,
    touch: bool,
) {
    let gap = 3.0;
    let left = 28.0;
    let room = ui.available_width() - left;
    let weeks = if touch {
        ((room / (TOUCH_CELL + gap)).floor() as usize).max(1)
    } else if ui.available_width() >= WIDE {
        26
    } else {
        17
    };
    let cell = (room / weeks as f32 - gap).clamp(8.0, if touch { f32::MAX } else { 18.0 });
    let size = vec2(
        left + weeks as f32 * (cell + gap),
        7.0 * (cell + gap) + 16.0 + 6.0 + cell,
    );
    let picked_id = ui.id().with((id, "picked_day"));
    let mut picked: Option<Date> = ui.data(|d| d.get_temp(picked_id)).flatten();
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    let start = crate::stats::week_start(today)
        .checked_sub(jiff::Span::new().weeks(weeks as i64 - 1))
        .unwrap_or(today);
    for (row, name) in ["Mon", "", "Wed", "", "Fri", "", "Sun"].iter().enumerate() {
        painter.text(
            pos2(
                rect.left(),
                rect.top() + 16.0 + row as f32 * (cell + gap) + cell / 2.0,
            ),
            egui::Align2::LEFT_CENTER,
            *name,
            egui::FontId::proportional(10.0),
            ui.visuals().weak_text_color(),
        );
    }
    let mut hovered = None;
    let mut total = 0;
    for week in 0..weeks {
        for weekday in 0..7 {
            let Ok(day) = start.checked_add(jiff::Span::new().days(week as i64 * 7 + weekday))
            else {
                continue;
            };
            if day > today {
                continue;
            }
            let x = rect.left() + left + week as f32 * (cell + gap);
            let y = rect.top() + 16.0 + weekday as f32 * (cell + gap);
            if day.day() <= 7 && weekday == 0 {
                painter.text(
                    pos2(x, rect.top()),
                    egui::Align2::LEFT_TOP,
                    day.strftime("%b").to_string(),
                    egui::FontId::proportional(10.0),
                    ui.visuals().weak_text_color(),
                );
            }
            let ms = days.get(&day_key(day)).copied().unwrap_or(0);
            total += ms;
            let square = Rect::from_min_size(pos2(x, y), vec2(cell, cell));
            let response = ui.interact(
                square.expand(gap / 2.0),
                ui.id().with((id, week, weekday)),
                Sense::click(),
            );
            if response.clicked() {
                picked = if picked == Some(day) { None } else { Some(day) };
            }
            if response.hovered() {
                hovered = Some((day, ms));
            }
            level_cell(
                ui,
                &painter,
                square,
                ms,
                response.hovered() || picked == Some(day),
            );
        }
    }
    ui.data_mut(|d| d.insert_temp(picked_id, picked));
    legend(
        ui,
        &painter,
        pos2(rect.left(), rect.top() + 16.0 + 7.0 * (cell + gap) + 6.0),
        cell,
    );
    let text = match hovered.or(picked.map(|d| (d, days.get(&day_key(d)).copied().unwrap_or(0)))) {
        Some((day, ms)) => format!("{}: {}", day.strftime("%a %-d %b"), play_time(ms)),
        None => format!("Last {weeks} weeks: {}", play_time(total)),
    };
    readout(ui, text);
}

pub(super) fn legend(ui: &Ui, painter: &egui::Painter, top_left: egui::Pos2, side: f32) {
    let font = egui::FontId::proportional(11.0);
    let color = ui.visuals().weak_text_color();
    let middle = top_left.y + side / 2.0;
    let text = painter.text(
        pos2(top_left.x, middle),
        egui::Align2::LEFT_CENTER,
        "0 h",
        font.clone(),
        color,
    );
    let mut x = text.right() + 4.0;
    for ms in [0, 1, LEVELS[0], LEVELS[1], LEVELS[2]] {
        let square = Rect::from_min_size(pos2(x, top_left.y), vec2(side, side));
        level_cell(ui, painter, square, ms, false);
        x += side + 3.0;
    }
    painter.text(
        pos2(x + 1.0, middle),
        egui::Align2::LEFT_CENTER,
        "2 h or more",
        font,
        color,
    );
}

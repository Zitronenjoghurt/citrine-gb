use super::chart::day_bars;
use super::*;
use egui::{Rect, pos2};

#[test]
fn play_time_reads_as_hours_and_minutes() {
    assert_eq!(play_time(42_000), "0 min");
    assert_eq!(play_time(5 * 60_000), "5 min");
    assert_eq!(play_time(185 * 60_000), "3 h 05 min");
    assert_eq!(play_time(130 * 3_600_000 + 60_000), "130 h 01 min");
}

#[test]
fn chart_scale_rounds_up() {
    assert_eq!(short_time(scale_top(0)), "10 min");
    assert_eq!(short_time(scale_top(47 * 60_000)), "50 min");
    assert_eq!(short_time(scale_top(111 * 60_000)), "2 h");
    assert_eq!(short_time(scale_top(120 * 60_000)), "2 h");
    assert_eq!(short_time(scale_top(47 * 3_600_000)), "50 h");
}

#[test]
fn sessions_say_one_session() {
    assert_eq!(sessions(1), "1 session");
    assert_eq!(sessions(1253), "1,253 sessions");
}

#[test]
fn counts_group_thousands() {
    assert_eq!(count(7), "7");
    assert_eq!(count(1253), "1,253");
    assert_eq!(count(1_000_000), "1,000,000");
}

#[test]
fn week_bars_run_monday_to_sunday() {
    let saturday = Date::new(2026, 10, 31).unwrap();
    let labels: Vec<String> = day_bars(&BTreeMap::new(), saturday, Range::Week)
        .into_iter()
        .map(|bar| bar.label)
        .collect();
    assert_eq!(labels, ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]);
}

fn tap_calendar(edge: bool) -> Option<Date> {
    let saturday = Date::new(2026, 10, 31).unwrap();
    let days = BTreeMap::new();
    let ctx = egui::Context::default();
    let mut picked_id = None;
    let mut target = egui::Pos2::ZERO;
    for step in 0..6 {
        let mut input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(412.0, 900.0))),
            ..Default::default()
        };
        match step {
            2 => input.events.push(egui::Event::PointerMoved(target)),
            3 | 4 => input.events.push(egui::Event::PointerButton {
                pos: target,
                button: egui::PointerButton::Primary,
                pressed: step == 3,
                modifiers: Default::default(),
            }),
            _ => {}
        }
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let start = ui.cursor().min;
                let room = ui.available_width() - 28.0;
                let weeks = ((room / (TOUCH_CELL + 3.0)).floor() as usize).max(1);
                let cell = room / weeks as f32 - 3.0;
                let x = start.x + 28.0 + (weeks - 1) as f32 * (cell + 3.0);
                let y = start.y + 16.0 + 2.0 * (cell + 3.0) + cell / 2.0;
                target = pos2(if edge { x + 0.5 } else { x + cell / 2.0 }, y);
                picked_id = Some(ui.id().with(("t", "picked_day")));
                calendar(ui, "t", &days, saturday, true);
            });
        });
    }
    ctx.data(|d| d.get_temp::<Option<Date>>(picked_id.unwrap()))
        .flatten()
}

#[test]
fn tapping_a_calendar_day_picks_it() {
    let wednesday = Date::new(2026, 10, 28).unwrap();
    assert_eq!(tap_calendar(false), Some(wednesday));
    assert_eq!(tap_calendar(true), Some(wednesday));
}

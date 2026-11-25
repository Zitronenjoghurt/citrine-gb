use crate::app::Citrine;
use crate::app::tests::{load_game, phone_app, shown_whole, text_rect};
use egui::Context;

fn card_behind(ctx: &Context, output: &egui::FullOutput, row: egui::Rect) -> Option<egui::Rect> {
    let fill = super::kit::card_fill(&ctx.style().visuals);
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(rect) if rect.fill == fill && rect.rect.contains(row.center()) => {
                Some(rect.rect)
            }
            _ => None,
        })
        .min_by(|a, b| a.area().total_cmp(&b.area()))
}

fn open_hold(app: &mut Citrine) {
    let start = web_time::Instant::now();
    let progress = crate::progress::Progress::unknown();
    for at in [0, 1] {
        app.toasts.load(
            "hold",
            "Getting your save from Steam Deck",
            progress,
            start + std::time::Duration::from_secs(at),
        );
    }
    app.toasts.load_note(
        "hold",
        "The game starts when it arrives, or after 4 seconds",
    );
}

#[test]
fn on_a_phone_and_a_big_screen_a_page_toast_never_covers_a_row_at_any_scroll_position() {
    for (big, size) in [
        (false, egui::vec2(412.0, 915.0)),
        (true, egui::vec2(1280.0, 720.0)),
    ] {
        let name = format!("page-toast-{big}");
        let mut app = phone_app(&name);
        app.ui.settings.big_screen.on = big;
        load_game(&mut app, include_bytes!("../../../homebrew/tobu.gb"), None);
        app.emulator.running = false;
        let ctx = Context::default();
        let middle = egui::pos2(size.x / 2.0, size.y / 2.0);
        let wheel = |down: f32| {
            vec![
                egui::Event::PointerMoved(middle),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -down),
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        let frame = |app: &mut Citrine, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                events,
                ..Default::default()
            };
            ctx.run(input, |ctx| {
                super::touch_spacing(ctx, true);
                app.render(ctx);
                app.show_toasts(ctx);
            })
        };
        let last = |output: &egui::FullOutput| {
            text_rect(output, "Second screens").expect("the page's last row")
        };
        let covered = |app: &Citrine, output: &egui::FullOutput, when: &str| {
            let mut texts = 0;
            for toast in app.toasts.rects(&ctx) {
                for clipped in &output.shapes {
                    let egui::Shape::Text(text) = &clipped.shape else {
                        continue;
                    };
                    let shown = text.visual_bounding_rect().intersect(clipped.clip_rect);
                    if !shown.is_positive() || toast.contains_rect(shown) {
                        continue;
                    }
                    texts += 1;
                    assert!(
                        !toast.intersects(shown),
                        "{name}, {when}: the toast {toast:?} covers {:?} at {shown:?}",
                        text.galley.text()
                    );
                }
            }
            texts
        };
        for _ in 0..40 {
            let _ = frame(&mut app, Vec::new());
        }
        for at in 0..40 {
            let _ = frame(&mut app, if at == 0 { wheel(5000.0) } else { Vec::new() });
        }
        let end = last(&frame(&mut app, Vec::new()));
        for at in 0..40 {
            let _ = frame(&mut app, if at == 0 { wheel(-5000.0) } else { Vec::new() });
        }
        let scrolls = (last(&frame(&mut app, Vec::new())).top() - end.top()).abs() > 1.0;
        open_hold(&mut app);
        assert!(!app.mobile.nav.on_game(), "{name}: the Games page shows");
        let mut checked = 0;
        for at in 0..40 {
            let output = frame(&mut app, Vec::new());
            assert_eq!(app.toasts.rects(&ctx).len(), 1, "{name}");
            checked += covered(&app, &output, &format!("opening, frame {at}"));
        }
        let toast = app.toasts.rects(&ctx)[0];
        let room = crate::app::toasts::strip_room(&ctx);
        let gap = room - toast.height() - 8.0;
        assert!(
            (gap - 8.0).abs() < 1.0,
            "{name}: the strip holds the toast with 8 pt above and below, {gap} above"
        );
        for at in 0..60 {
            let output = frame(&mut app, if at % 3 == 0 { wheel(90.0) } else { Vec::new() });
            checked += covered(&app, &output, &format!("scrolling, frame {at}"));
        }
        for at in 0..40 {
            let _ = frame(&mut app, if at == 0 { wheel(5000.0) } else { Vec::new() });
        }
        let output = frame(&mut app, Vec::new());
        checked += covered(&app, &output, "at the page end");
        let toast = app.toasts.rects(&ctx)[0];
        assert!(
            last(&output).bottom() < toast.top(),
            "{name}: the last row {:?} sits above the toast {toast:?}",
            last(&output)
        );
        let card = card_behind(&ctx, &output, last(&output))
            .unwrap_or_else(|| panic!("{name}: no card behind the last row"));
        let above = toast.top() - card.bottom();
        assert!(
            if scrolls {
                (above - 8.0).abs() <= 1.0
            } else {
                above >= 7.0
            },
            "{name}: at the page end the card ends {above} pt above the toast, 8 below it, scrolls {scrolls}"
        );
        if !big {
            assert!(
                (toast.left() - super::kit::SIDE).abs() < 0.5
                    && (size.x - super::kit::SIDE - toast.right()).abs() < 0.5,
                "{name}: the toast {toast:?} keeps to the card column"
            );
        }
        assert!(checked > 100, "{name}: the page shows, {checked} texts");
        let mut row = last(&output).bottom();
        app.toasts.load_done("hold");
        let mut most = 0.0_f32;
        for at in 0..90 {
            let output = frame(&mut app, Vec::new());
            covered(&app, &output, &format!("closing, frame {at}"));
            let now = last(&output).bottom();
            most = most.max((now - row).abs());
            row = now;
        }
        assert!(app.toasts.rects(&ctx).is_empty(), "{name}: the toast went");
        assert!(
            most <= 4.0,
            "{name}: the strip eases away, the page moved {most} pt in one frame"
        );
        assert!(
            (row - end.bottom()).abs() < 0.5,
            "{name}: the last row settles at {row}, where it ended before, {}",
            end.bottom()
        );
    }
}

#[test]
fn on_a_big_screen_a_page_at_its_end_keeps_its_end_when_a_toast_opens() {
    let size = egui::vec2(1280.0, 600.0);
    let mut app = phone_app("page-toast-keeps-end");
    app.ui.settings.big_screen.on = true;
    load_game(&mut app, include_bytes!("../../../homebrew/tobu.gb"), None);
    app.emulator.running = false;
    let ctx = Context::default();
    let middle = egui::pos2(size.x / 2.0, size.y / 2.0);
    let frame = |app: &mut Citrine, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        };
        ctx.run(input, |ctx| {
            super::touch_spacing(ctx, true);
            app.render(ctx);
            app.show_toasts(ctx);
        })
    };
    for _ in 0..40 {
        let _ = frame(&mut app, Vec::new());
    }
    let resting = text_rect(&frame(&mut app, Vec::new()), "Second screens").map(|row| row.bottom());
    for at in 0..40 {
        let events = if at == 0 {
            vec![
                egui::Event::PointerMoved(middle),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -5000.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        } else {
            Vec::new()
        };
        let _ = frame(&mut app, events);
    }
    let output = frame(&mut app, Vec::new());
    assert!(shown_whole(&output, "Second screens"), "the page end shows");
    let scrolled = text_rect(&output, "Second screens").map(|row| row.bottom());
    assert!(
        resting.is_none_or(|before| scrolled.is_some_and(|after| after < before - 1.0)),
        "the page scrolls to its end: the last row ended at {resting:?}, now at {scrolled:?}"
    );
    open_hold(&mut app);
    assert!(!app.mobile.nav.on_game(), "the Games page shows");
    for at in 0..60 {
        let output = frame(&mut app, Vec::new());
        for row in ["Homebrew games", "Second screens"] {
            assert!(
                shown_whole(&output, row),
                "frame {at}: {row:?} is cut while the toast opens"
            );
        }
    }
    let output = frame(&mut app, Vec::new());
    let toast = app.toasts.rects(&ctx)[0];
    let last = text_rect(&output, "Second screens").expect("the page's last row");
    let card = card_behind(&ctx, &output, last).expect("a card behind the last row");
    let strip = egui::containers::panel::PanelState::load(&ctx, egui::Id::new("toast_room"))
        .expect("the toast's strip")
        .rect;
    let above = toast.top() - card.bottom();
    let below = strip.bottom() - toast.bottom();
    assert!(
        (above - 8.0).abs() <= 1.0 && (below - 8.0).abs() <= 1.0 && (above - below).abs() <= 1.0,
        "the last card ends {above} pt above the toast, the page {below} pt below it: card {card:?}, toast {toast:?}, strip {strip:?}"
    );
}

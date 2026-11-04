use super::{Ask, CodeAsk, ask, confirm, confirm_with_code, destructive};

#[test]
fn enter_in_the_code_dialog_confirms() {
    let ctx = egui::Context::default();
    let mut code = "123456".to_owned();
    let mut frame = |events: Vec<egui::Event>| {
        let mut choice = None;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(412.0, 800.0),
            )),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            choice = confirm_with_code(
                ctx,
                "code_dialog_test",
                CodeAsk {
                    message: "Type a code",
                    code: Some(&mut code),
                    problem: None,
                    action: "Turn off",
                },
            );
        });
        choice
    };
    assert_eq!(frame(Vec::new()), None);
    assert_eq!(frame(Vec::new()), None);
    let enter = egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    assert_eq!(frame(vec![enter]), Some(true));
}

#[test]
fn only_actions_that_lose_something_are_red() {
    for action in ["Delete", "Delete everything", "Forget", "Reset", "Turn off"] {
        assert!(destructive(action), "{action} should be red");
    }
    for action in ["Sign out", "Import", "Load", "Trust", "Restart"] {
        assert!(!destructive(action), "{action} should not be red");
    }
}

#[test]
fn an_ask_with_a_load_shows_its_bar() {
    let ctx = egui::Context::default();
    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(412.0, 800.0),
        )),
        ..Default::default()
    };
    let notes = Vec::new();
    let mut texts = Vec::new();
    for _ in 0..2 {
        let output = ctx.run(input(), |ctx| {
            let _ = ask(
                ctx,
                "load_dialog_test",
                Ask {
                    message: "Delete everything on this device?",
                    notes: &notes,
                    action: "Delete everything",
                    action_enabled: false,
                    extra: Some(("Export first", false)),
                    load: Some((
                        "Exporting your data",
                        crate::progress::Progress::bytes(1024 * 1024, 4 * 1024 * 1024),
                    )),
                },
            );
        });
        texts = output
            .shapes
            .into_iter()
            .filter_map(|clipped| match clipped.shape {
                egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect();
    }
    assert!(
        texts.contains(&"Exporting your data".to_owned()),
        "{texts:?}"
    );
    assert!(texts.contains(&"1.0 of 4.0 MB".to_owned()), "{texts:?}");
}

#[test]
fn a_desktop_confirm_fills_its_action_red_only_when_it_loses_something() {
    let fills = |action: &str| {
        let ctx = egui::Context::default();
        let input = |frame: u32| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            time: Some(f64::from(frame) * 0.1),
            ..Default::default()
        };
        let mut output = None;
        for frame in 0..10 {
            output = Some(ctx.run(input(frame), |ctx| {
                let _ = confirm(ctx, "desktop_fill_test", "Go on?", action);
            }));
        }
        let fills: Vec<egui::Color32> = output
            .unwrap()
            .shapes
            .into_iter()
            .filter_map(|clipped| match clipped.shape {
                egui::Shape::Rect(rect) => Some(rect.fill),
                _ => None,
            })
            .collect();
        let visuals = ctx.style().visuals.clone();
        (
            fills.contains(&crate::app::ui_state::ui_theme::danger(&visuals)),
            fills.contains(&crate::app::ui_state::ui_theme::brand(&visuals)),
        )
    };
    assert_eq!(fills("Reset"), (true, false));
    assert_eq!(fills("Load"), (false, true));
}

#[test]
fn a_phone_confirm_centres_its_question_and_note_over_cancel() {
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(412.0, 915.0));
    let input = |frame: u32| egui::RawInput {
        screen_rect: Some(screen),
        time: Some(f64::from(frame) * 0.1),
        ..Default::default()
    };
    let mut output = None;
    for frame in 0..20 {
        output = Some(ctx.run(input(frame), |ctx| {
            crate::app::mobile::sheet::set_area(ctx, Some(screen));
            let _ = super::confirm_with_detail(
                ctx,
                "phone_centre_test",
                "Reset the game?",
                Some("Unsaved progress is lost."),
                "Reset",
            );
        }));
    }
    let centres: Vec<(String, f32)> = output
        .unwrap()
        .shapes
        .into_iter()
        .filter_map(|clipped| match clipped.shape {
            egui::Shape::Text(text) => Some((
                text.galley.text().to_owned(),
                text.visual_bounding_rect().center().x,
            )),
            _ => None,
        })
        .collect();
    let centre = |wanted: &str| {
        centres
            .iter()
            .find(|(t, _)| t == wanted)
            .map(|(_, x)| *x)
            .unwrap_or_else(|| panic!("no {wanted} in {centres:?}"))
    };
    let cancel = centre("Cancel");
    for line in ["Reset the game?", "Unsaved progress is lost.", "Reset"] {
        let x = centre(line);
        assert!(
            (x - cancel).abs() < 2.0,
            "{line} at {x}, Cancel at {cancel}"
        );
    }
}

#[test]
fn a_desktop_confirm_centres_its_question_note_and_buttons() {
    let ctx = egui::Context::default();
    let input = |frame: u32| egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 800.0),
        )),
        time: Some(f64::from(frame) * 0.1),
        ..Default::default()
    };
    let mut output = None;
    for frame in 0..10 {
        output = Some(ctx.run(input(frame), |ctx| {
            let _ = super::confirm_with_detail(
                ctx,
                "desktop_centre_test",
                "Reset the game?",
                Some("Unsaved progress is lost."),
                "Reset",
            );
        }));
    }
    let texts: Vec<(String, egui::Rect)> = output
        .unwrap()
        .shapes
        .into_iter()
        .filter_map(|clipped| match clipped.shape {
            egui::Shape::Text(text) => {
                Some((text.galley.text().to_owned(), text.visual_bounding_rect()))
            }
            _ => None,
        })
        .collect();
    let rect = |wanted: &str| {
        texts
            .iter()
            .find(|(t, _)| t == wanted)
            .map(|(_, r)| *r)
            .unwrap_or_else(|| panic!("no {wanted} in {texts:?}"))
    };
    let question = rect("Reset the game?").center().x;
    let note = rect("Unsaved progress is lost.").center().x;
    let row = (rect("Reset").center().x + rect("Cancel").center().x) / 2.0;
    for (what, x) in [("note", note), ("buttons", row)] {
        assert!(
            (x - question).abs() < 3.0,
            "{what} at {x}, question at {question}"
        );
    }
}

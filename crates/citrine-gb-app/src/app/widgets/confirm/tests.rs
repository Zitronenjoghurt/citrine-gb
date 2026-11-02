use super::{CodeAsk, confirm_with_code};

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

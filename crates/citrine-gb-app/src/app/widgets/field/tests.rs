use super::code_field;

fn enter() -> egui::Event {
    egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

fn frames(focus: bool, inputs: Vec<Vec<egui::Event>>) -> Vec<bool> {
    let ctx = egui::Context::default();
    let mut code = "123456".to_owned();
    inputs
        .into_iter()
        .map(|events| {
            let mut entered = false;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 300.0),
                )),
                events,
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    entered = code_field(ui, &mut code, "Code", 200.0, focus);
                });
            });
            entered
        })
        .collect()
}

#[test]
fn enter_submits_a_code_field_that_keeps_its_focus() {
    let entered = frames(true, vec![vec![], vec![], vec![enter()], vec![]]);
    assert_eq!(entered, [false, false, true, false]);
}

#[test]
fn a_focused_code_field_submits_nothing_without_enter() {
    let entered = frames(true, vec![vec![], vec![], vec![], vec![]]);
    assert_eq!(entered, [false; 4]);
}

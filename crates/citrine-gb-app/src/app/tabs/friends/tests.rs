use super::*;

fn mia() -> Friend {
    Friend {
        online_id: OnlineId::new(7).unwrap(),
        display_name: "Mia Sommer".into(),
        public_key: citrine_gb_net::PublicKey([0; 32]),
        state: FriendState::Friend,
        presence: None,
    }
}

fn row_frame(ctx: &egui::Context, events: Vec<egui::Event>, fold: bool) -> (bool, RowClicks) {
    let mode = if fold { RowMode::Folded } else { RowMode::Chat };
    let mut shown = false;
    let mut clicks = RowClicks::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(217.0, 300.0),
        )),
        events,
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            clicks = person_row(ui, &mia(), None, 3, mode, "Bring the cable", |ui| {
                shown = true;
                let _ = ui.button(icons::DOTS_THREE);
            });
        });
    });
    (shown, clicks)
}

#[test]
fn the_add_row_and_the_rows_under_it_fit_a_thin_dock() {
    let mut online = Online::default();
    for width in [120.0, 148.0, 180.0, 300.0] {
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(2.0);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 600.0),
            )),
            ..Default::default()
        };
        let mut state = FriendsUi::default();
        let mut inner = egui::Rect::NOTHING;
        let mut shapes = Vec::new();
        for _ in 0..2 {
            shapes = ctx
                .run(input.clone(), |ctx| {
                    egui::SidePanel::left("dock")
                        .exact_width(width)
                        .show(ctx, |ui| {
                            inner = ui.max_rect();
                            add_row(ui, &mut online, &mut state);
                            person_row(
                                ui,
                                &mia(),
                                None,
                                0,
                                RowMode::Folded,
                                "Last seen 12 min ago",
                                |_| {},
                            );
                        });
                })
                .shapes;
        }
        let hint = shapes.iter().any(|clipped| {
            matches!(&clipped.shape, egui::Shape::Text(t) if t.galley.text() == "Online ID")
        });
        assert!(hint, "{width}: the field shows its hint");
        for clipped in &shapes {
            let rect = clipped.shape.visual_bounding_rect();
            if !rect.is_positive() || rect.left() < inner.left() || rect.width() < 1.5 {
                continue;
            }
            assert!(
                rect.right() <= inner.right() + 0.5,
                "{width}: {:?} ends at {} past {}",
                clipped.shape,
                rect.right(),
                inner.right()
            );
        }
    }
}

fn press(button: egui::PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: egui::pos2(100.0, 25.0),
        button,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn a_folded_row_shows_its_buttons_only_under_the_pointer() {
    let ctx = egui::Context::default();
    assert!(!row_frame(&ctx, vec![], true).0);
    let over = vec![egui::Event::PointerMoved(egui::pos2(100.0, 25.0))];
    assert!(row_frame(&ctx, over, true).0);
    let away = vec![egui::Event::PointerMoved(egui::pos2(100.0, 250.0))];
    assert!(!row_frame(&ctx, away, true).0);
    assert!(row_frame(&ctx, vec![], false).0);
}

#[test]
fn a_folded_row_shows_its_buttons_while_it_has_keyboard_focus() {
    let ctx = egui::Context::default();
    assert!(!row_frame(&ctx, vec![], true).0);
    let tab = egui::Event::Key {
        key: egui::Key::Tab,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    row_frame(&ctx, vec![tab], true);
    assert!(row_frame(&ctx, vec![], true).0);
}

#[test]
fn a_click_on_the_row_opens_the_chat_and_a_right_click_its_menu() {
    let ctx = egui::Context::default();
    let over = vec![egui::Event::PointerMoved(egui::pos2(100.0, 25.0))];
    row_frame(&ctx, over, true);
    row_frame(&ctx, vec![press(egui::PointerButton::Primary, true)], true);
    let (_, clicks) = row_frame(&ctx, vec![press(egui::PointerButton::Primary, false)], true);
    assert!(clicks.open && !clicks.menu && !clicks.face);
    row_frame(
        &ctx,
        vec![press(egui::PointerButton::Secondary, true)],
        true,
    );
    let (_, clicks) = row_frame(
        &ctx,
        vec![press(egui::PointerButton::Secondary, false)],
        true,
    );
    assert!(clicks.menu && !clicks.open);
}

#[test]
fn last_seen_reads_plainly() {
    assert_eq!(ago(1_000, 1_030), "just now");
    assert_eq!(ago(0, 7_200), "2 h ago");
    assert_eq!(ago(0, 90_000), "yesterday");
    assert_eq!(
        ago(1_761_825_600, 1_761_825_600 + 30 * 86_400),
        "30 Oct 2025"
    );
}

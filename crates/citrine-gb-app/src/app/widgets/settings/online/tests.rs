use super::storage::storage_row;

#[test]
fn a_storage_header_opens_from_its_label() {
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0));
    let mut clicked = false;
    let mut frame = |events: Vec<egui::Event>| {
        let mut at = egui::Pos2::ZERO;
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                at = ui.cursor().min + egui::vec2(40.0, ui.spacing().interact_size.y / 2.0);
                clicked |= storage_row(ui, Some(false), "Cloud saves (1)", Some(4096));
            });
        });
        at
    };
    let at = frame(Vec::new());
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame(vec![egui::Event::PointerMoved(at)]);
    frame(vec![button(true)]);
    frame(vec![button(false)]);
    assert!(clicked, "a click on the row's label at {at:?} did nothing");
}

fn device(id: u64, online: bool, last_seen: u64) -> citrine_gb_net::DeviceInfo {
    citrine_gb_net::DeviceInfo {
        id,
        name: format!("Device {id}"),
        added: 1,
        last_seen,
        online,
    }
}

#[test]
fn this_device_comes_first_then_online_ones_then_the_latest_seen() {
    let list = [
        device(1, false, 50),
        device(2, true, 10),
        device(3, false, 90),
        device(4, true, 20),
    ];
    let order: Vec<u64> = super::devices::in_order(&list, Some(1))
        .iter()
        .map(|d| d.id)
        .collect();
    assert_eq!(order, [1, 4, 2, 3]);
}

#[test]
fn signing_out_warns_about_the_relay_id_only_without_two_factor() {
    let on = super::devices::sign_out_question("Firefox on Mac", true);
    assert_eq!(
        on,
        "Sign out Firefox on Mac? It forgets this account and its chats here."
    );
    let off = super::devices::sign_out_question("Firefox on Mac", false);
    assert!(off.starts_with(&on));
    assert!(off.ends_with("Two-factor sign-in stops that."));
}

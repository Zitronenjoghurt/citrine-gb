use super::*;

fn keys(n: u64) -> Vec<ItemKey> {
    (0..n).map(ItemKey::Chat).collect()
}

#[test]
fn arrows_step_through_the_list_and_stop_at_its_ends() {
    let list = keys(3);
    let mut state = ViewerState::default();
    state.open(Source::Media, ItemKey::Chat(1));
    assert!(state.step(&list, 1));
    assert_eq!(state.current(), Some(&ItemKey::Chat(2)));
    assert!(!state.step(&list, 1), "the last item stays");
    assert!(state.step(&list, -1));
    assert!(state.step(&list, -1));
    assert!(!state.step(&list, -1), "the first item stays");
    assert_eq!(state.current(), Some(&ItemKey::Chat(0)));
}

#[test]
fn the_arrow_keys_step_and_space_plays() {
    let ctx = Context::default();
    let press = |key| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    };
    let input = egui::RawInput {
        events: vec![
            press(Key::ArrowRight),
            press(Key::ArrowLeft),
            press(Key::Space),
        ],
        ..Default::default()
    };
    let mut seen = Vec::new();
    let _ = ctx.run(input, |ctx| seen = keys_pressed(ctx, true));
    assert_eq!(seen, [Act::Step(-1), Act::Step(1), Act::Toggle]);
    let input = egui::RawInput {
        events: vec![press(Key::Space)],
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| seen = keys_pressed(ctx, false));
    assert!(seen.is_empty(), "space is left alone for a picture");
}

#[test]
fn escape_closes_the_viewer() {
    let ctx = Context::default();
    let mut state = ViewerState::default();
    state.open(Source::Media, ItemKey::Chat(0));
    let frame = |events: Vec<egui::Event>, state: &mut ViewerState| {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            let modal = egui::Modal::new(Id::new("media_viewer"))
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| ui.label("shown"));
            if modal.should_close() {
                state.close();
            }
        });
    };
    frame(Vec::new(), &mut state);
    assert!(state.is_open());
    assert_eq!(depth(&state), 1);
    let escape = egui::Event::Key {
        key: Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    };
    frame(vec![escape], &mut state);
    assert!(!state.is_open());
    assert_eq!(depth(&state), 0);
}

#[test]
fn a_swipe_steps_and_a_pull_down_closes() {
    assert_eq!(swipe(vec2(-90.0, 10.0)), Some(Act::Step(1)));
    assert_eq!(swipe(vec2(90.0, -5.0)), Some(Act::Step(-1)));
    assert_eq!(swipe(vec2(10.0, 150.0)), Some(Act::Close));
    assert_eq!(swipe(vec2(20.0, 30.0)), None);
    assert_eq!(swipe(vec2(5.0, -150.0)), None);
}

#[test]
fn pixels_scale_by_whole_steps_when_big() {
    let room = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 700.0));
    let rect = fit(vec2(160.0, 144.0), room, 1.0);
    assert_eq!(rect.size(), vec2(640.0, 576.0));
    assert_eq!(rect.center(), room.center());
    let small = Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 300.0));
    assert_eq!(
        fit(vec2(160.0, 144.0), small, 1.0).width(),
        300.0 * 160.0 / 160.0
    );
    let phone = fit(vec2(160.0, 144.0), small, 2.625);
    let pixels = phone.width() * 2.625;
    assert!((pixels - pixels.round()).abs() < 1e-3);
    assert_eq!(pixels.round(), 640.0);
}

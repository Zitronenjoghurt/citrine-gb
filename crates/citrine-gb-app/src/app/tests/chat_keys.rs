use super::*;
use crate::app::ui_state::keybinds::GbButton;
use citrine_gb_net::{Friend, FriendState, OnlineId, Presence, PublicKey};

fn ana() -> OnlineId {
    OnlineId::new(7).unwrap()
}

fn chatting_while_playing(name: &str) -> Citrine {
    let mut app = phone_app(name);
    app.ui.settings.layout = LayoutMode::Desktop;
    playing(&mut app);
    app.online.befriend_for_test(Friend {
        online_id: ana(),
        display_name: "Ana".into(),
        public_key: PublicKey([4; 32]),
        state: FriendState::Friend,
        presence: Some(Presence::Online { away: false }),
    });
    app.ui.friends.chat.open(ana());
    app.open_tab(Tab::Friends);
    app
}

fn frame(app: &mut Citrine, ctx: &Context, events: Vec<egui::Event>) -> egui::FullOutput {
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 800.0),
        )),
        events,
        ..Default::default()
    };
    app.raw_keys(ctx, &mut input);
    ctx.run(input, |ctx| {
        mobile::touch_spacing(ctx, false);
        app.render(ctx);
        app.handle_snapshot_hotkeys(ctx);
        let Citrine {
            emulator, pads, ui, ..
        } = app;
        emulator.handle_input(
            ctx,
            pads.as_mut(),
            &ui.settings.bindings,
            &mut ui.capture,
            None,
        );
        app.handle_event_queue(false);
    })
}

fn key(key: egui::Key, pressed: bool) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: Default::default(),
    }
}

fn text(text: &str) -> egui::Event {
    egui::Event::Text(text.into())
}

fn up_held(app: &Citrine) -> bool {
    app.emulator.is_held(GbButton::Up.key())
}

fn in_field(ctx: &Context) -> bool {
    ctx.wants_keyboard_input()
}

fn draft(app: &Citrine) -> Option<String> {
    app.ui
        .friends
        .chat
        .composing()
        .map(|(_, draft)| draft.to_owned())
}

fn press_tab(app: &mut Citrine, ctx: &Context) {
    frame(app, ctx, vec![key(egui::Key::Tab, true)]);
    frame(app, ctx, vec![key(egui::Key::Tab, false)]);
    frame(app, ctx, vec![]);
}

#[test]
fn tab_moves_the_cursor_to_the_chat_and_its_hint_says_so() {
    let mut app = chatting_while_playing("chat-key-tab");
    let ctx = Context::default();
    frame(&mut app, &ctx, vec![]);
    let out = frame(&mut app, &ctx, vec![]);
    assert!(text_containing(&out, "Tab to chat").is_some());
    assert!(text_containing(&out, "go back to the game").is_none());
    assert!(!in_field(&ctx));
    press_tab(&mut app, &ctx);
    assert!(in_field(&ctx));
    assert_eq!(draft(&app).as_deref(), Some(""));
    let out = frame(&mut app, &ctx, vec![]);
    assert!(text_containing(&out, "to chat").is_none());
    assert!(text_containing(&out, "Tab or Esc to go back to the game").is_some());
    assert_eq!(
        app.ui.settings.hotkeys.key_name(HotkeyAction::FocusChat),
        Some("Tab".to_owned())
    );
}

#[test]
fn enter_sends_and_hands_the_keys_straight_back_to_the_game() {
    let mut app = chatting_while_playing("chat-key-enter");
    let ctx = Context::default();
    frame(&mut app, &ctx, vec![]);
    press_tab(&mut app, &ctx);
    frame(&mut app, &ctx, vec![text("gg")]);
    assert_eq!(draft(&app).as_deref(), Some("gg"));
    frame(&mut app, &ctx, vec![key(egui::Key::Enter, true)]);
    assert!(!in_field(&ctx));
    assert_eq!(draft(&app).as_deref(), Some(""));
    let sent = app.online.conversation(ana()).unwrap().entries.len();
    assert_eq!(sent, 1);
    assert!(
        !app.emulator.is_held(GbButton::Start.key()),
        "the Enter that sent never reaches the game"
    );
    frame(&mut app, &ctx, vec![key(egui::Key::W, true)]);
    assert!(up_held(&app));
}

#[test]
fn esc_leaves_the_field_and_keeps_the_draft_and_the_chat() {
    let mut app = chatting_while_playing("chat-key-esc");
    let ctx = Context::default();
    frame(&mut app, &ctx, vec![]);
    press_tab(&mut app, &ctx);
    frame(&mut app, &ctx, vec![text("half a")]);
    frame(&mut app, &ctx, vec![key(egui::Key::Escape, true)]);
    assert!(!in_field(&ctx));
    assert_eq!(draft(&app).as_deref(), Some("half a"));
    assert_eq!(app.ui.friends.chat.current(), Some(ana()));
    assert!(!app.ui.settings.focus_mode);
    frame(&mut app, &ctx, vec![key(egui::Key::W, true)]);
    assert!(up_held(&app));
}

#[test]
fn a_key_held_while_moving_to_the_chat_lets_go_of_the_game() {
    let mut app = chatting_while_playing("chat-key-held");
    let ctx = Context::default();
    frame(&mut app, &ctx, vec![]);
    frame(&mut app, &ctx, vec![key(egui::Key::W, true)]);
    assert!(up_held(&app));
    press_tab(&mut app, &ctx);
    assert!(in_field(&ctx));
    assert!(!up_held(&app), "Up lets go once the chat has the keys");
    frame(&mut app, &ctx, vec![key(egui::Key::W, false)]);
    frame(&mut app, &ctx, vec![key(egui::Key::Escape, true)]);
    assert!(!in_field(&ctx));
    frame(&mut app, &ctx, vec![]);
    assert!(!up_held(&app));
    let repeat = egui::Event::Key {
        key: egui::Key::W,
        physical_key: None,
        pressed: true,
        repeat: true,
        modifiers: Default::default(),
    };
    frame(&mut app, &ctx, vec![repeat]);
    assert!(!up_held(&app), "a key repeat is not a new press");
}

#[test]
fn clicking_the_game_leaves_the_field() {
    let mut app = chatting_while_playing("chat-key-click");
    let ctx = Context::default();
    frame(&mut app, &ctx, vec![]);
    press_tab(&mut app, &ctx);
    frame(&mut app, &ctx, vec![text("brb")]);
    frame(&mut app, &ctx, tap(egui::pos2(1000.0, 400.0)));
    frame(&mut app, &ctx, vec![]);
    assert!(!in_field(&ctx));
    assert_eq!(draft(&app).as_deref(), Some("brb"));
}

#[test]
fn tab_again_goes_back_to_the_game_with_the_draft_kept() {
    let mut app = chatting_while_playing("chat-key-tab-out");
    let ctx = Context::default();
    frame(&mut app, &ctx, vec![]);
    press_tab(&mut app, &ctx);
    assert!(in_field(&ctx));
    frame(&mut app, &ctx, vec![text("one sec")]);
    press_tab(&mut app, &ctx);
    assert!(!in_field(&ctx));
    assert_eq!(draft(&app).as_deref(), Some("one sec"));
    frame(&mut app, &ctx, vec![key(egui::Key::W, true)]);
    assert!(up_held(&app));
    press_tab(&mut app, &ctx);
    assert!(in_field(&ctx));
    assert_eq!(draft(&app).as_deref(), Some("one sec"));
}

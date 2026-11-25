use super::*;
use citrine_gb::ReadMemory;

pub(super) fn dev_app(name: &str, fake: &Fake, hardcore: bool) -> Citrine {
    let mut app = app(name, fake);
    app.ui.settings.dev_mode = true;
    app.ui.settings.achievements.hardcore = hardcore;
    app
}

pub(super) fn with_set(app: &mut Citrine, ctx: &Context) {
    load_zero_rom(app);
    app.events.open_tab(Tab::Achievements);
    app.handle_event_queue(true);
    signed_in(app, ctx);
    for _ in 0..10 {
        if app.achievements.game.is_some() {
            break;
        }
        frame(app, ctx, vec![]);
    }
    assert!(app.achievements.game.is_some());
    frame(app, ctx, vec![]);
}

pub(super) fn rc_frames(app: &mut Citrine, frames: usize) {
    for _ in 0..frames {
        app.emulator.with_achievements(|seat| {
            let (client, mut memory) = seat.parts();
            client.do_frame(&mut memory);
        });
    }
}

pub(super) fn poke(app: &mut Citrine, address: u16, value: u8) {
    app.emulator
        .as_gb_mut()
        .unwrap()
        .memory
        .write_naive(address, value);
}

fn peek(app: &Citrine, address: u16) -> u8 {
    app.emulator.as_gb().unwrap().memory.read_naive(address)
}

fn unlocked(app: &mut Citrine, id: u32) -> bool {
    app.emulator
        .with_achievements(|seat| seat.client.game())
        .flatten()
        .into_iter()
        .flat_map(|game| game.groups)
        .flat_map(|group| group.achievements)
        .any(|achievement| achievement.id == id && achievement.unlocked)
}

fn awards(fake: &Fake) -> Vec<String> {
    let sent = fake.sent.lock().unwrap();
    sent.iter()
        .filter_map(|(request, _)| request.post_data.clone())
        .filter(|data| data.starts_with("r=awardachievement"))
        .collect()
}

#[test]
fn hardcore_stays_off_and_hidden_without_dev_mode() {
    let ctx = Context::default();
    let fake = fake();
    let mut app = app("ra-hc-dev", &fake);
    app.ui.settings.achievements.hardcore = true;
    with_set(&mut app, &ctx);
    let shown = texts(&frame(&mut app, &ctx, vec![]));
    assert!(!app.emulator.hardcore());
    assert!(!shown.iter().any(|t| t == "Hardcore"), "{shown:?}");

    app.ui.settings.dev_mode = true;
    frame(&mut app, &ctx, vec![]);
    let shown = texts(&frame(&mut app, &ctx, vec![]));
    assert!(app.emulator.hardcore());
    assert!(shown.iter().any(|t| t == "Hardcore"), "{shown:?}");
}

#[test]
fn loading_a_state_is_refused_in_hardcore_and_saving_still_works() {
    let ctx = Context::default();
    let fake = fake();
    let mut app = dev_app("ra-hc-load", &fake, true);
    with_set(&mut app, &ctx);
    assert!(app.emulator.hardcore());
    poke(&mut app, 0xC100, 0x11);
    app.emulator.save_snapshot(1).unwrap();
    poke(&mut app, 0xC100, 0x55);

    assert!(!app.emulator.load_snapshot(1).unwrap());
    assert_eq!(peek(&app, 0xC100), 0x55);

    app.ui.settings.achievements.hardcore = false;
    frame(&mut app, &ctx, vec![]);
    assert!(!app.emulator.hardcore());
    assert!(app.emulator.load_snapshot(1).unwrap());
    assert_eq!(peek(&app, 0xC100), 0x11);
}

#[test]
fn rewind_frame_advance_and_link_play_are_off_in_hardcore() {
    let ctx = Context::default();
    let fake = fake();
    let mut app = dev_app("ra-hc-blocks", &fake, true);
    with_set(&mut app, &ctx);
    assert!(!app.emulator.can_rewind());
    assert!(!app.emulator.can_clip());

    app.emulator.running = false;
    let before = app.emulator.as_gb().unwrap().cpu.pc;
    app.emulator
        .step(citrine_gb::debug::step::Step::Instruction);
    assert_eq!(app.emulator.as_gb().unwrap().cpu.pc, before);

    let rom = Rom::new(&[0u8; 0x8000]);
    let linked = app
        .emulator
        .start_link(&rom, citrine_gb::console::ConsoleKind::Gb, None);
    assert!(linked.is_err());
    assert!(!app.emulator.linked());
}

#[test]
fn the_cheats_pane_says_cheats_are_off_and_can_turn_hardcore_off() {
    let ctx = Context::default();
    let fake = fake();
    let mut app = dev_app("ra-hc-cheats", &fake, true);
    with_set(&mut app, &ctx);
    app.events.open_tab(Tab::Cheats);
    app.handle_event_queue(true);
    let output = frame(&mut app, &ctx, vec![]);
    let shown = texts(&output);
    assert!(
        shown.iter().any(|t| t == "Cheats are off in hardcore"),
        "{shown:?}"
    );

    let button = rect_of(&output, "Turn hardcore off").center();
    frame(&mut app, &ctx, crate::app::tests::tap(button));
    frame(&mut app, &ctx, vec![]);
    assert!(!app.ui.settings.achievements.hardcore);
    assert!(!app.emulator.hardcore());
}

#[test]
fn turning_hardcore_on_mid_game_asks_then_restarts_and_unlocks_count_as_hardcore() {
    let ctx = Context::default();
    let fake = fake();
    let mut app = dev_app("ra-hc-on", &fake, false);
    with_set(&mut app, &ctx);
    assert!(!app.emulator.hardcore());

    let output = frame(&mut app, &ctx, vec![]);
    let switch = rect_of(&output, "Hardcore").center();
    frame(&mut app, &ctx, crate::app::tests::tap(switch));
    let output = frame(&mut app, &ctx, vec![]);
    let shown = texts(&output);
    assert!(shown.iter().any(|t| t == "Turn on hardcore?"), "{shown:?}");
    assert!(!app.ui.settings.achievements.hardcore);

    let restart = rect_of(&output, "Restart in hardcore").center();
    frame(&mut app, &ctx, crate::app::tests::tap(restart));
    frame(&mut app, &ctx, vec![]);
    frame(&mut app, &ctx, vec![]);
    assert!(app.ui.settings.achievements.hardcore);
    assert!(app.emulator.hardcore());

    unlock_surge(&mut app);
    for _ in 0..4 {
        frame(&mut app, &ctx, vec![]);
    }
    let awards = awards(&fake);
    assert_eq!(awards.len(), 1, "{awards:?}");
    assert!(awards[0].contains("&h=1"), "{awards:?}");
}

#[test]
fn a_pause_right_after_a_pause_waits_until_hardcore_allows_it() {
    let ctx = Context::default();
    let fake = fake();
    let mut app = dev_app("ra-hc-pause", &fake, true);
    with_set(&mut app, &ctx);
    app.emulator.running = true;
    app.emulator.toggle_running();
    assert!(!app.emulator.running);
    app.emulator.toggle_running();
    assert!(app.emulator.running);
    rc_frames(&mut app, 5);

    app.emulator.toggle_running();
    assert!(app.emulator.running);
    assert_eq!(app.emulator.pause_due(), Some(15));
    let shown = settle(&mut app, &ctx, "Hardcore limits pauses. Pausing in 1 s.");
    assert!(
        shown
            .iter()
            .any(|t| t == "Hardcore limits pauses. Pausing in 1 s."),
        "{shown:?}"
    );

    rc_frames(&mut app, 10);
    app.emulator.follow_pause_due();
    assert!(app.emulator.running);
    rc_frames(&mut app, 5);
    app.emulator.follow_pause_due();
    assert!(!app.emulator.running);
    assert_eq!(app.emulator.pause_due(), None);
}

#[test]
fn loading_a_state_puts_back_the_achievement_progress_it_was_saved_with() {
    let ctx = Context::default();
    let fake = Fake::default();
    fake.answer("r=login2", 200, LOGIN);
    let hits = game_set().replace("0xHc012=9", "0xHc012=9.3.");
    fake.answer("r=achievementsets", 200, &hits);
    fake.answer("r=startsession", 200, NO_UNLOCKS);
    fake.answer("r=awardachievement", 200, AWARD);
    let mut app = app("ra-hc-progress", &fake);
    with_set(&mut app, &ctx);
    poke(&mut app, 0xC012, 0);
    rc_frames(&mut app, 1);
    let hit = |app: &mut Citrine| {
        poke(app, 0xC012, 9);
        rc_frames(app, 1);
        poke(app, 0xC012, 0);
        rc_frames(app, 1);
    };
    hit(&mut app);
    app.emulator.save_snapshot(1).unwrap();
    hit(&mut app);
    assert!(!unlocked(&mut app, 5502));

    assert!(app.emulator.load_snapshot(1).unwrap());
    hit(&mut app);
    assert!(
        !unlocked(&mut app, 5502),
        "the state's single hit came back"
    );
    hit(&mut app);
    assert!(unlocked(&mut app, 5502));
}

#[test]
fn hardcore_notice_closes_up_its_badge_slot() {
    for phone in [false, true] {
        let card = card(Notice::Hardcore, None, phone);
        assert_eq!(card.side, 0.0);
        assert!(card.detail.chars().count() <= 46, "{}", card.detail);
    }
}

use super::hardcore::{dev_app, poke, rc_frames, with_set};
use super::*;
use crate::achievements::boards::{Best, NEEDS_HARDCORE, NOT_SENT, View};
use crate::app::ui_state::settings::LayoutMode;

const BOARD: &str = concat!(
    r#"{"ID":7001,"Title":"Floors","Description":"Climb as high as you can","#,
    r#""Mem":"STA:0xHc013=1::CAN:0xHc013=2::SUB:0xHc013=3::VAL:0xHc014","#,
    r#""Format":"VALUE","LowerIsBetter":false,"Hidden":false}"#
);
const RANKINGS: &str = concat!(
    r#"{"Success":true,"LeaderboardData":{"LBID":7001,"LBFormat":"VALUE","LowerIsBetter":0,"#,
    r#""LBTitle":"Floors","LBDesc":"Climb as high as you can","LBMem":"","GameID":1234,"#,
    r#""LBAuthor":"User1","LBCreated":"2013-10-20 22:12:21","LBUpdated":"2021-06-14 08:18:19","#,
    r#""Entries":[{"User":"Speedy","Rank":1,"Index":1,"Score":99,"DateSubmitted":1615654895},"#,
    r#"{"User":"ClaraPlays","Rank":2,"Index":2,"Score":42,"DateSubmitted":1615654896}],"#,
    r#""TotalEntries":3}}"#
);

fn boards_fake() -> Fake {
    let fake = Fake::default();
    fake.answer("r=login2", 200, LOGIN);
    let set = game_set().replace(
        r#""Leaderboards":[]"#,
        &format!(r#""Leaderboards":[{BOARD}]"#),
    );
    fake.answer("r=achievementsets", 200, &set);
    fake.answer("r=startsession", 200, NO_UNLOCKS);
    fake.answer("r=lbinfo", 200, RANKINGS);
    fake
}

fn lbinfo(fake: &Fake) -> Vec<String> {
    let sent = fake.sent.lock().unwrap();
    sent.iter()
        .filter_map(|(request, _)| request.post_data.clone())
        .filter(|post| post.starts_with("r=lbinfo"))
        .collect()
}

fn leaderboards_tab(app: &mut Citrine, ctx: &Context) -> Vec<String> {
    let output = frame(app, ctx, vec![]);
    let tab = rect_of(&output, "Leaderboards").center();
    frame(app, ctx, crate::app::tests::tap(tab));
    settle(app, ctx, "Floors")
}

fn desk(app: &mut Citrine, ctx: &Context, events: Vec<egui::Event>) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 800.0),
        )),
        events,
        ..Default::default()
    };
    ctx.run(input, |ctx| {
        app.follow_achievements(ctx);
        app.render(ctx);
        app.show_toasts(ctx);
        app.handle_event_queue(false);
    })
}

fn desk_until(app: &mut Citrine, ctx: &Context, until: &str) -> egui::FullOutput {
    let mut output = desk(app, ctx, vec![]);
    for _ in 0..12 {
        if texts(&output).iter().any(|t| t == until) {
            break;
        }
        output = desk(app, ctx, vec![]);
    }
    output
}

#[test]
fn the_leaderboards_tab_lists_the_boards_and_asks_for_each_best() {
    let ctx = Context::default();
    let fake = boards_fake();
    let mut app = dev_app("ra-boards-list", &fake, true);
    with_set(&mut app, &ctx);
    let shown = leaderboards_tab(&mut app, &ctx);
    assert!(shown.iter().any(|t| t == "Floors"), "{shown:?}");
    assert!(shown.iter().any(|t| t == NOT_SENT), "{shown:?}");
    assert!(shown.iter().all(|t| t != NEEDS_HARDCORE), "{shown:?}");
    settle(&mut app, &ctx, "never");
    let asked = lbinfo(&fake);
    assert!(
        asked
            .iter()
            .any(|post| post.contains("i=7001") && post.ends_with("&c=1")),
        "{asked:?}"
    );
    let best = &app.achievements.boards.list[0].best;
    assert!(
        matches!(best, Best::Score(entry) if entry.rank == 2 && entry.score == "42"),
        "{best:?}"
    );
}

#[test]
fn softcore_says_leaderboards_need_hardcore_mode() {
    let ctx = Context::default();
    let fake = boards_fake();
    let mut app = dev_app("ra-boards-softcore", &fake, false);
    with_set(&mut app, &ctx);
    let shown = leaderboards_tab(&mut app, &ctx);
    assert!(shown.iter().any(|t| t == NEEDS_HARDCORE), "{shown:?}");
    assert!(shown.iter().all(|t| t != NOT_SENT), "{shown:?}");
}

#[test]
fn without_dev_mode_there_is_no_leaderboards_tab() {
    let ctx = Context::default();
    let fake = boards_fake();
    let mut app = app("ra-boards-no-dev", &fake);
    with_set(&mut app, &ctx);
    assert!(app.achievements.boards.tabs());
    let shown = texts(&frame(&mut app, &ctx, vec![]));
    assert!(shown.iter().all(|t| t != "Leaderboards"), "{shown:?}");
    assert!(shown.iter().all(|t| t != NEEDS_HARDCORE), "{shown:?}");
    let shown = texts(&desk(&mut app, &ctx, vec![]));
    assert!(shown.iter().all(|t| t != "Leaderboards"), "{shown:?}");

    app.ui.settings.dev_mode = true;
    let shown = texts(&frame(&mut app, &ctx, vec![]));
    assert!(shown.iter().any(|t| t == "Leaderboards"), "{shown:?}");
}

#[test]
fn a_desktop_board_opens_its_rankings_and_esc_steps_back() {
    let ctx = Context::default();
    let fake = boards_fake();
    let mut app = dev_app("ra-boards-desktop", &fake, true);
    with_set(&mut app, &ctx);
    app.ui.settings.layout = LayoutMode::Desktop;
    app.events.open_tab(Tab::Achievements);
    app.handle_event_queue(false);
    app.achievements.boards.view = View::Leaderboards;
    let output = desk_until(&mut app, &ctx, "Floors");
    let row = rect_of(&output, "Floors").center();
    desk(&mut app, &ctx, crate::app::tests::tap(row));
    let output = desk_until(&mut app, &ctx, "Speedy");
    let shown = texts(&output);
    assert!(shown.iter().any(|t| t == "Speedy"), "{shown:?}");
    assert!(shown.iter().any(|t| t == "ClaraPlays"), "{shown:?}");
    let asked = lbinfo(&fake);
    assert!(
        asked.iter().any(|post| post.ends_with("i=7001&c=10")),
        "{asked:?}"
    );
    desk(
        &mut app,
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
    );
    let shown = texts(&desk_until(&mut app, &ctx, "Floors"));
    assert_eq!(app.achievements.boards.open, None);
    assert!(shown.iter().any(|t| t == "Floors"), "{shown:?}");
    assert!(shown.iter().all(|t| t != "Speedy"), "{shown:?}");
}

#[test]
fn an_attempt_shows_a_notice_and_a_tracker_until_it_fails() {
    let ctx = Context::default();
    let fake = boards_fake();
    let mut app = dev_app("ra-boards-attempt", &fake, true);
    with_set(&mut app, &ctx);
    rc_frames(&mut app, 1);
    poke(&mut app, 0xC014, 7);
    poke(&mut app, 0xC013, 1);
    rc_frames(&mut app, 3);
    let shown = settle(&mut app, &ctx, "Leaderboard attempt started");
    assert!(
        shown.iter().any(|t| t == "Leaderboard attempt started"),
        "{shown:?}"
    );
    assert_eq!(
        app.achievements.boards.tracker_lines(),
        vec![("Floors".to_owned(), "7".to_owned())]
    );
    poke(&mut app, 0xC013, 2);
    rc_frames(&mut app, 3);
    let shown = settle(&mut app, &ctx, "Leaderboard attempt failed");
    assert!(
        shown.iter().any(|t| t == "Leaderboard attempt failed"),
        "{shown:?}"
    );
    assert!(app.achievements.boards.tracker_lines().is_empty());
}

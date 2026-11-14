use super::Citrine;
use crate::achievements::{Leaving, Notice};
use crate::app::toasts::{Card, Toast};
use egui::{Context, TextureHandle};

const DESKTOP_BADGE: f32 = 36.0;
const PHONE_BADGE: f32 = 48.0;

impl Citrine {
    pub(super) fn follow_achievements(&mut self, ctx: &Context) {
        let paused = !self.emulator.is_running();
        let notices = self
            .achievements
            .pump(&mut self.emulator, &mut self.ui.settings, paused);
        let phone = self.is_mobile(ctx);
        for (notice, badge) in self.achievements.follow(ctx, notices) {
            self.toasts.add(Toast::card(card(notice, badge, phone)));
        }
        if ctx.input(|i| i.viewport().close_requested())
            && !self.achievements.quit_anyway
            && self.held_by_unsent(Leaving::Quit)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
    }

    pub(super) fn held_by_unsent(&mut self, leaving: Leaving) -> bool {
        if self.achievements.unsent() == 0 {
            return false;
        }
        self.achievements.leaving = Some(leaving);
        true
    }

    pub(super) fn unsent_confirm(&mut self, ctx: &Context) {
        let Some(leaving) = self.achievements.leaving else {
            return;
        };
        let unsent = self.achievements.unsent();
        let answer = if unsent == 0 {
            Some(true)
        } else {
            let (leave, stay) = match leaving {
                Leaving::Quit => ("Quit anyway", "Keep Citrine open"),
                Leaving::CloseGame => ("Close anyway", "Keep playing"),
            };
            crate::app::widgets::confirm::confirm_with_cancel(
                ctx,
                "unsent_unlocks",
                &unsent_text(unsent),
                Some(unsent_detail(leaving, unsent)),
                leave,
                stay,
            )
        };
        let Some(leave) = answer else {
            return;
        };
        self.achievements.leaving = None;
        if !leave {
            return;
        }
        match leaving {
            Leaving::Quit => {
                self.achievements.quit_anyway = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Leaving::CloseGame => self.close_game(),
        }
    }
}

fn unsent_text(unsent: usize) -> String {
    match unsent {
        1 => "Citrine hasn't sent 1 achievement to RetroAchievements yet".to_owned(),
        n => format!("Citrine hasn't sent {n} achievements to RetroAchievements yet"),
    }
}

fn unsent_detail(leaving: Leaving, unsent: usize) -> &'static str {
    match (leaving, unsent) {
        (Leaving::Quit, 1) => "Quitting now loses it.",
        (Leaving::Quit, _) => "Quitting now loses them.",
        (Leaving::CloseGame, _) => {
            "Citrine keeps trying after you close the game, until you quit the app."
        }
    }
}

fn points(points: u32) -> String {
    match points {
        1 => "1 point".to_owned(),
        n => format!("{n} points"),
    }
}

fn card(notice: Notice, badge: Option<TextureHandle>, phone: bool) -> Card {
    let side = if phone { PHONE_BADGE } else { DESKTOP_BADGE };
    let (title, detail, aside) = match notice {
        Notice::Unlocked(achievement) if phone => (
            "Achievement unlocked".to_owned(),
            achievement.title,
            points(achievement.points),
        ),
        Notice::Unlocked(achievement) => (
            achievement.title,
            achievement.description,
            format!("Unlocked · {}", points(achievement.points)),
        ),
        Notice::Completed { title, .. } => (
            "Every achievement unlocked".to_owned(),
            title,
            String::new(),
        ),
    };
    Card {
        badge,
        side,
        title,
        detail,
        aside,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::achievements::Achievements;
    use crate::achievements::net::fake::Fake;
    use crate::app::tabs::Tab;
    use citrine_gb::WriteMemory;
    use citrine_gb::gb::GbModel;
    use citrine_gb::rom::Rom;
    use citrine_gb_achievements::Achievement;

    const LOGIN: &str = r#"{"Success":true,"User":"ClaraPlays","Token":"ApiToken","Score":1000,"SoftcoreScore":240,"Messages":0,"Permissions":1,"AccountType":"Registered"}"#;
    const NO_UNLOCKS: &str = r#"{"Success":true,"Unlocks":[],"HardcoreUnlocks":[]}"#;
    const AWARD: &str = r#"{"Success":true,"Score":1000,"SoftcoreScore":245,"AchievementID":5501,"AchievementsRemaining":1}"#;

    fn game_set() -> String {
        String::from(concat!(
            r#"{"Success":true,"GameId":1234,"Title":"Sample Game","ConsoleId":4,"#,
            r#""ImageIconUrl":"http://server/Images/112233.png","#,
            r#""RichPresenceGameId":1234,"RichPresencePatch":"","Sets":[{"#,
            r#""AchievementSetId":1111,"GameId":1234,"Title":null,"Type":"core","#,
            r#""ImageIconUrl":"http://server/Images/112233.png","Achievements":["#,
            r#"{"ID":5501,"Title":"Sunny Thunder Badge","Description":"Defeat Lt. Surge.","Flags":3,"Points":5,"#,
            r#""MemAddr":"0xHc010=3_0xHc011=7","Author":"User1","BadgeName":"120704","#,
            r#""Created":1367266583,"Modified":1376929305},"#,
            r#"{"ID":5502,"Title":"Journey Has Started","Description":"Obtain the Pokedex.","Flags":3,"Points":2,"#,
            r#""MemAddr":"0xHc012=9","Author":"User1","BadgeName":"120736","#,
            r#""Created":1376970283,"Modified":1376970283}"#,
            r#"],"Leaderboards":[]}]}"#
        ))
    }

    fn fake() -> Fake {
        let fake = Fake::default();
        fake.answer("r=login2", 200, LOGIN);
        fake.answer("r=achievementsets", 200, &game_set());
        fake.answer("r=startsession", 200, NO_UNLOCKS);
        fake.answer("r=awardachievement", 200, AWARD);
        fake
    }

    fn app(name: &str, fake: &Fake) -> Citrine {
        let mut app = crate::app::tests::phone_app(name);
        let cache =
            std::env::temp_dir().join(format!("citrine-ra-cache-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&cache);
        app.achievements = Box::new(Achievements::on_net(
            Box::new(fake.clone()),
            crate::storage::Backend::at(cache),
        ));
        app.events.open_tab(Tab::Achievements);
        app.handle_event_queue(true);
        app
    }

    fn frame(app: &mut Citrine, ctx: &Context, events: Vec<egui::Event>) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(412.0, 915.0),
            )),
            events,
            ..Default::default()
        };
        ctx.run(input, |ctx| {
            let touch = app.is_mobile(ctx);
            crate::app::mobile::touch_spacing(ctx, touch);
            app.follow_achievements(ctx);
            app.render(ctx);
            app.unsent_confirm(ctx);
            app.show_toasts(ctx);
            app.handle_event_queue(touch);
        })
    }

    fn texts(output: &egui::FullOutput) -> Vec<String> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => Some(text.galley.text().trim().to_owned()),
                _ => None,
            })
            .collect()
    }

    fn rect_of(output: &egui::FullOutput, ends: &str) -> egui::Rect {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text().trim().ends_with(ends) => {
                    Some(text.visual_bounding_rect())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("no {ends:?} in {:?}", texts(output)))
    }

    fn signed_in(app: &mut Citrine, ctx: &Context) -> egui::FullOutput {
        frame(app, ctx, vec![]);
        app.achievements.form.username = "ClaraPlays".to_owned();
        app.achievements.form.password = "Pa$$word".to_owned();
        let output = frame(app, ctx, vec![]);
        let button = rect_of(&output, " Sign in").center();
        frame(app, ctx, crate::app::tests::tap(button));
        let mut output = frame(app, ctx, vec![]);
        for _ in 0..10 {
            if app.achievements.user().is_some() {
                break;
            }
            output = frame(app, ctx, vec![]);
        }
        output
    }

    fn unlock_surge(app: &mut Citrine) {
        for step in 0..3 {
            if step == 1 {
                let gb = app.emulator.as_gb_mut().unwrap();
                gb.memory.write_naive(0xC010, 3);
                gb.memory.write_naive(0xC011, 7);
            }
            app.emulator.with_achievements(|seat| {
                let (client, mut memory) = seat.parts();
                client.do_frame(&mut memory);
            });
        }
    }

    fn load_zero_rom(app: &mut Citrine) {
        app.emulator
            .load_rom(&Rom::new(&[0u8; 0x8000]), GbModel::Dmg, None)
            .expect("load");
    }

    #[test]
    fn signing_in_keeps_the_token_and_never_the_password() {
        let ctx = Context::default();
        let fake = fake();
        let mut app = app("ra-sign-in", &fake);
        let output = frame(&mut app, &ctx, vec![]);
        assert!(
            texts(&output)
                .iter()
                .any(|t| t == "Sign in to RetroAchievements"),
            "{:?}",
            texts(&output)
        );
        let output = signed_in(&mut app, &ctx);
        let account = app.ui.settings.achievements.account.clone().unwrap();
        assert_eq!(account.username, "ClaraPlays");
        assert_eq!(account.token, "ApiToken");
        assert!(app.achievements.form.password.is_empty());
        let saved = serde_json::to_string(&app.ui.settings).unwrap();
        assert!(!saved.contains("Pa$$word"));
        let output = if texts(&output).iter().any(|t| t == "1,240 points in total") {
            output
        } else {
            frame(&mut app, &ctx, vec![])
        };
        assert!(
            texts(&output).iter().any(|t| t == "1,240 points in total"),
            "{:?}",
            texts(&output)
        );
        assert!(texts(&output).iter().all(|t| !t.contains("Sign in to")));
    }

    #[test]
    fn the_desktop_sign_in_is_one_centred_column() {
        let ctx = Context::default();
        let fake = fake();
        let mut app = app("ra-desktop-sign-in", &fake);
        app.ui.settings.layout = crate::app::ui_state::settings::LayoutMode::Desktop;
        app.events.open_tab(Tab::Achievements);
        app.handle_event_queue(false);
        app.achievements.form.username = "ClaraPlays".to_owned();
        app.achievements.form.password = "Pa$$word".to_owned();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        };
        let mut output = None;
        for _ in 0..4 {
            output = Some(ctx.run(input.clone(), |ctx| {
                app.render(ctx);
                app.handle_event_queue(false);
            }));
        }
        let output = output.unwrap();
        let card = ctx
            .data(|d| {
                d.get_temp::<egui::Rect>(egui::Id::new(crate::app::tabs::achievements::SIGN_IN_ID))
            })
            .expect("a sign in card");
        let button = rect_of(&output, " Sign in");
        let heading = rect_of(&output, "Sign in to RetroAchievements");
        let fill = crate::app::ui_state::ui_theme::brand(&ctx.style().visuals);
        let filled = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(rect) if rect.fill == fill && card.contains_rect(rect.rect) => {
                    Some(rect.rect)
                }
                _ => None,
            })
            .max_by(|a, b| a.width().total_cmp(&b.width()))
            .expect("a filled Sign in button");
        assert!(filled.width() > card.width() - 40.0, "{card:?} {filled:?}");
        assert!((200.0..=300.0).contains(&card.width()), "{card:?}");
        assert!(
            (button.center().x - card.center().x).abs() <= 2.0,
            "{card:?} {button:?}"
        );
        assert!(
            (heading.center().x - card.center().x).abs() <= 2.0,
            "{card:?} {heading:?}"
        );
        assert!(heading.top() > 200.0, "{heading:?}");
    }

    const RED_HASH: &str = "3d45c1ee9abd5738df46d2bdda8b57dc";

    fn library_fake() -> Fake {
        let fake = fake();
        fake.answer(
            "r=systemgames&s=4",
            200,
            concat!(
                r#"{"Success":true,"Response":[{"ID":724,"Title":"Pokemon Red Version","#,
                r#""ImageIcon":"/Images/1.png","ImageUrl":"server/Images/1.png","#,
                r#""NumAchievements":52,"NumLeaderboards":0,"Points":400,"#,
                r#""SupportedHashes":["3D45C1EE9ABD5738DF46D2BDDA8B57DC"]}]}"#
            ),
        );
        fake.answer(
            "r=systemgames&s=6",
            200,
            r#"{"Success":true,"Response":[]}"#,
        );
        fake.answer(
            "r=allprogress",
            200,
            r#"{"Success":true,"Response":{"724":{"Achievements":52,"Unlocked":12}}}"#,
        );
        fake
    }

    fn desktop_games(app: &mut Citrine, ctx: &Context) -> Vec<String> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        };
        let mut output = None;
        for _ in 0..3 {
            output = Some(ctx.run(input.clone(), |ctx| {
                app.render(ctx);
                app.handle_event_queue(false);
            }));
        }
        texts(&output.unwrap())
    }

    #[test]
    fn the_library_shows_progress_while_signed_in_and_switched_on() {
        use crate::app::ui_state::library::{Identity, Opened};
        use crate::app::widgets::game_tile::Progress;
        use citrine_gb::console::ConsoleKind;
        let ctx = Context::default();
        let fake = library_fake();
        let mut app = app("ra-library", &fake);
        let path = "/roms/red.gb";
        app.ui.library.record_opened(
            Opened {
                path: path.to_owned(),
                name: "Pokemon Red".to_owned(),
                console: Some(ConsoleKind::Gb),
                rom_key: Some("red-key".to_owned()),
                ..Opened::default()
            },
            1,
        );
        app.ui.library.identify(
            path,
            Identity {
                rom_key: "red-key".to_owned(),
                sha1: "red-sha1".to_owned(),
                crc32: None,
                ra_hash: Some(RED_HASH.to_owned()),
                console: Some(ConsoleKind::Gb),
                name: None,
            },
        );
        let settings = app.ui.settings.achievements.clone();
        assert_eq!(app.achievements.progress(&settings, Some(RED_HASH)), None);
        signed_in(&mut app, &ctx);
        for _ in 0..10 {
            if app
                .achievements
                .progress(&settings, Some(RED_HASH))
                .is_some()
            {
                break;
            }
            frame(&mut app, &ctx, vec![]);
        }
        assert_eq!(
            app.achievements.progress(&settings, Some(RED_HASH)),
            Some(Progress {
                earned: 12,
                total: 52
            })
        );
        let sent: Vec<String> = fake
            .sent
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(request, _)| request.post_data.clone())
            .collect();
        for asked in ["r=systemgames&s=4", "r=systemgames&s=6"] {
            assert!(sent.iter().any(|post| post.starts_with(asked)), "{sent:?}");
        }
        for console in ["&c=4", "&c=6"] {
            assert!(
                sent.iter()
                    .any(|post| post.starts_with("r=allprogress") && post.ends_with(console)),
                "{sent:?}"
            );
        }

        app.ui.settings.layout = crate::app::ui_state::settings::LayoutMode::Desktop;
        app.ui.games.info = Some(path.to_owned());
        app.events.open_tab(Tab::Games);
        app.handle_event_queue(false);
        let tile = |shown: &[String]| shown.iter().any(|t| t.ends_with(" 12 of 52"));
        let row = |shown: &[String]| {
            shown.iter().any(|t| t == "Achievements") && shown.iter().any(|t| t == "12 of 52")
        };
        let shown = desktop_games(&mut app, &ctx);
        assert!(tile(&shown) && row(&shown), "{shown:?}");

        app.ui.settings.achievements.in_library = false;
        let shown = desktop_games(&mut app, &ctx);
        assert!(!tile(&shown) && !row(&shown), "{shown:?}");

        app.ui.settings.achievements.in_library = true;
        app.achievements.sign_out();
        frame(&mut app, &ctx, vec![]);
        assert!(app.achievements.user().is_none());
        let shown = desktop_games(&mut app, &ctx);
        assert!(!tile(&shown) && !row(&shown), "{shown:?}");

        let asked = |fake: &Fake| {
            fake.sent
                .lock()
                .unwrap()
                .iter()
                .filter(|(request, _)| {
                    request
                        .post_data
                        .as_deref()
                        .is_some_and(|post| post.starts_with("r=allprogress"))
                })
                .count()
        };
        let before = asked(&fake);
        app.achievements.form.username = "ClaraPlays".to_owned();
        app.achievements.form.password = "Pa$$word".to_owned();
        app.achievements.sign_in();
        for _ in 0..10 {
            if asked(&fake) > before {
                break;
            }
            frame(&mut app, &ctx, vec![]);
        }
        assert!(app.achievements.user().is_some());
        assert_eq!(asked(&fake), before + 2);
    }

    #[test]
    fn a_saved_token_signs_in_by_itself() {
        let ctx = Context::default();
        let fake = Fake::default();
        fake.answer("r=login2&u=ClaraPlays&t=ApiToken", 200, LOGIN);
        let mut app = app("ra-token", &fake);
        app.ui.settings.achievements.account = Some(crate::achievements::Account {
            username: "ClaraPlays".to_owned(),
            token: "ApiToken".to_owned(),
        });
        for _ in 0..4 {
            frame(&mut app, &ctx, vec![]);
        }
        assert!(app.achievements.user().is_some());
        let sent = fake.sent.lock().unwrap();
        let (_, agent) = sent.first().unwrap();
        assert!(agent.starts_with("Citrine/"), "{agent}");
    }

    #[test]
    fn the_list_shows_the_set_and_an_unlock_shows_a_notice() {
        let ctx = Context::default();
        let fake = fake();
        let mut app = app("ra-list", &fake);
        load_zero_rom(&mut app);
        app.events.open_tab(Tab::Achievements);
        app.handle_event_queue(true);
        signed_in(&mut app, &ctx);
        for _ in 0..10 {
            if app.achievements.game.is_some() {
                break;
            }
            frame(&mut app, &ctx, vec![]);
        }
        let shown = texts(&frame(&mut app, &ctx, vec![]));
        assert!(shown.iter().any(|t| t == "0 of 2 unlocked"), "{shown:?}");
        assert!(shown.iter().any(|t| t == "Locked · 2"), "{shown:?}");
        assert!(
            shown.iter().any(|t| t == "Sunny Thunder Badge"),
            "{shown:?}"
        );

        unlock_surge(&mut app);
        let mut shown = Vec::new();
        for _ in 0..4 {
            shown = texts(&frame(&mut app, &ctx, vec![]));
            if shown.iter().any(|t| t == "Achievement unlocked") {
                break;
            }
        }
        assert!(
            shown.iter().any(|t| t == "Achievement unlocked"),
            "{shown:?}"
        );
        assert!(shown.iter().any(|t| t == "Unlocked · 1"), "{shown:?}");
        for _ in 0..4 {
            shown = texts(&frame(&mut app, &ctx, vec![]));
            if shown.iter().any(|t| t == "1,245 points in total") {
                break;
            }
        }
        assert!(
            shown.iter().any(|t| t == "1,245 points in total"),
            "{shown:?}"
        );
        let sent = fake.sent.lock().unwrap();
        assert!(sent.iter().any(|(request, _)| {
            request
                .post_data
                .as_deref()
                .unwrap_or("")
                .starts_with("r=awardachievement")
        }));
    }

    fn console_frame(app: &mut Citrine, ctx: &Context) -> bool {
        let mut away = false;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(412.0, 915.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            app.begin_console(ctx);
            away = app.emulator.core_is_away();
            let Citrine {
                emulator,
                pads,
                ui,
                play,
                ..
            } = app;
            emulator
                .update_linked(
                    ctx,
                    pads.as_mut(),
                    &ui.settings.bindings,
                    &mut ui.capture,
                    ui.controller_debug.log_mut(),
                    play.session_mut().filter(|s| s.is_live()),
                )
                .expect("frame");
        });
        away
    }

    #[test]
    fn the_pump_runs_while_the_console_thread_has_the_game() {
        let ctx = Context::default();
        let fake = Fake::default();
        fake.answer("r=login2&u=ClaraPlays&t=ApiToken", 200, LOGIN);
        fake.answer("r=achievementsets", 200, &game_set());
        fake.answer("r=startsession", 200, NO_UNLOCKS);
        let mut app = app("ra-lent", &fake);
        app.ui.settings.achievements.account = Some(crate::achievements::Account {
            username: "ClaraPlays".to_owned(),
            token: "ApiToken".to_owned(),
        });
        load_zero_rom(&mut app);
        assert!(app.emulator.has_worker());
        let mut lent = 0;
        for _ in 0..20 {
            if console_frame(&mut app, &ctx) {
                lent += 1;
            }
            if app.achievements.game.is_some() {
                break;
            }
        }
        assert!(lent > 0, "the core never went to the console thread");
        assert!(app.achievements.user().is_some(), "never signed in");
        assert!(app.achievements.game.is_some(), "the set never loaded");
    }

    #[test]
    fn closing_the_game_with_an_unsent_unlock_asks_and_waits_for_it() {
        let ctx = Context::default();
        let fake = Fake::default();
        fake.answer("r=login2", 200, LOGIN);
        fake.answer("r=achievementsets", 200, &game_set());
        fake.answer("r=startsession", 200, NO_UNLOCKS);
        let mut app = app("ra-unsent", &fake);
        load_zero_rom(&mut app);
        signed_in(&mut app, &ctx);
        for _ in 0..10 {
            if app.achievements.game.is_some() {
                break;
            }
            frame(&mut app, &ctx, vec![]);
        }
        unlock_surge(&mut app);
        for _ in 0..4 {
            frame(&mut app, &ctx, vec![]);
        }
        assert_eq!(app.achievements.unsent(), 1);

        app.ask_close_game();
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        let shown = texts(&output);
        assert!(
            shown
                .iter()
                .any(|t| t == "Citrine hasn't sent 1 achievement to RetroAchievements yet"),
            "{shown:?}"
        );
        assert!(shown.iter().any(|t| t == "Close anyway"), "{shown:?}");
        assert!(app.emulator.has_game());
        let wait = rect_of(&output, "Keep playing").center();
        frame(&mut app, &ctx, crate::app::tests::tap(wait));
        frame(&mut app, &ctx, vec![]);
        assert_eq!(app.achievements.leaving, None);
        assert!(app.emulator.has_game());

        app.ask_close_game();
        app.emulator.running = false;
        fake.answer("r=awardachievement", 200, AWARD);
        let until = web_time::Instant::now() + std::time::Duration::from_secs(10);
        while app.emulator.has_game() && web_time::Instant::now() < until {
            frame(&mut app, &ctx, vec![]);
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(app.achievements.unsent(), 0);
        assert!(!app.emulator.has_game());
        assert_eq!(app.achievements.leaving, None);
    }

    #[test]
    fn the_unsent_question_counts_the_unlocks() {
        assert_eq!(
            unsent_text(1),
            "Citrine hasn't sent 1 achievement to RetroAchievements yet"
        );
        assert_eq!(
            unsent_text(3),
            "Citrine hasn't sent 3 achievements to RetroAchievements yet"
        );
        assert!(unsent_detail(Leaving::Quit, 1).ends_with("loses it."));
        assert!(unsent_detail(Leaving::Quit, 2).ends_with("loses them."));
    }

    #[test]
    fn signing_out_leaves_nothing_of_the_account() {
        let ctx = Context::default();
        let fake = fake();
        let mut app = app("ra-sign-out", &fake);
        signed_in(&mut app, &ctx);
        app.achievements.sign_out();
        frame(&mut app, &ctx, vec![]);
        let shown = texts(&frame(&mut app, &ctx, vec![]));
        assert!(app.ui.settings.achievements.account.is_none());
        assert!(shown.iter().all(|t| !t.contains("ClaraPlays")), "{shown:?}");
        assert!(shown.iter().any(|t| t == "Sign in to RetroAchievements"));
    }

    fn surge() -> Notice {
        Notice::Unlocked(Achievement {
            title: "Sunny Thunder Badge".to_owned(),
            description: "Defeat Lt. Surge.".to_owned(),
            points: 5,
            ..Achievement::default()
        })
    }

    #[test]
    fn the_phone_notice_says_what_happened_over_the_name() {
        let card = card(surge(), None, true);
        assert_eq!(card.title, "Achievement unlocked");
        assert_eq!(card.detail, "Sunny Thunder Badge");
        assert_eq!(card.aside, "5 points");
        assert_eq!(card.side, PHONE_BADGE);
    }

    #[test]
    fn the_desktop_notice_has_the_name_and_the_description() {
        let card = card(surge(), None, false);
        assert_eq!(card.title, "Sunny Thunder Badge");
        assert_eq!(card.detail, "Defeat Lt. Surge.");
        assert_eq!(card.aside, "Unlocked · 5 points");
    }
}

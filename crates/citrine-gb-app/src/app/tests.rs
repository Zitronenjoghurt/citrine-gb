use super::*;
use crate::app::ui_state::hotkeys::HotkeyAction;
use crate::app::ui_state::keybinds::PadButton;
use crate::app::ui_state::settings::LayoutMode;
use crate::app::ui_state::settings::SettingsTab;
use citrine_gb::rom::Rom;
use citrine_gb::rom::header::RomCgbMode;
use mobile::{Sheet, TOUCH_TARGET_PX};

fn phone_app(name: &str) -> Citrine {
    let root =
        std::env::temp_dir().join(format!("citrine-touch-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut app = Citrine::default();
    app.emulator.store = crate::storage::SaveStore::at(root);
    app.ui.settings.layout = LayoutMode::Mobile;
    app
}

fn small_targets(app: &mut Citrine, ctx: &Context) -> Vec<String> {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(412.0, 3000.0),
        )),
        ..Default::default()
    };
    let mut output = None;
    for _ in 0..3 {
        output = Some(ctx.run(input.clone(), |ctx| {
            mobile::touch_spacing(ctx, app.is_mobile(ctx));
            app.render(ctx);
        }));
    }
    let link = ctx.style().visuals.hyperlink_color;
    let texts: Vec<(egui::Rect, String, bool)> = output
        .unwrap()
        .shapes
        .into_iter()
        .filter_map(|clipped| match clipped.shape {
            egui::Shape::Text(text) => Some((
                text.visual_bounding_rect(),
                text.galley.text().trim().to_owned(),
                text.fallback_color == link,
            )),
            _ => None,
        })
        .collect();
    ctx.viewport(|viewport| {
        viewport
            .prev_pass
            .widgets
            .layers()
            .flat_map(|(_, widgets)| widgets.iter())
            .filter(|widget| widget.sense.senses_click() && widget.sense.is_focusable())
            .filter(|widget| widget.rect.area() > 0.0)
            .filter(|widget| widget.rect.height() < TOUCH_TARGET_PX - 0.5)
            .filter_map(|widget| {
                let inside: Vec<&(egui::Rect, String, bool)> = texts
                    .iter()
                    .filter(|(rect, _, _)| widget.rect.contains(rect.center()))
                    .collect();
                let inline_link =
                    !inside.is_empty() && inside.iter().all(|(_, _, is_link)| *is_link);
                (!inline_link).then(|| {
                    let label: Vec<&str> =
                        inside.iter().map(|(_, text, _)| text.as_str()).collect();
                    format!(
                        "{} {:.0}px {:?}",
                        sheet_name(app),
                        widget.rect.height(),
                        label
                    )
                })
            })
            .collect()
    })
}

fn sheet_name(app: &Citrine) -> String {
    match app.mobile.sheet {
        Some(Sheet::Panel(Tab::Settings)) => {
            format!("Settings/{}", app.ui.settings.current_tab.title())
        }
        other => format!("{other:?}"),
    }
}

#[test]
fn every_touch_control_is_at_least_44_px_tall() {
    let ctx = Context::default();
    let mut app = phone_app("targets");
    let mut small = Vec::new();

    small.extend(small_targets(&mut app, &ctx));
    let mut rom = vec![0u8; 0x8000];
    rom[0x0147] = 0x03;
    rom[0x0149] = 0x02;
    app.emulator
        .load_rom(&citrine_gb::rom::Rom::new(&rom), GbModel::Dmg, None)
        .unwrap();
    app.emulator.save_snapshot(0).unwrap();
    app.ui.snapshots.invalidate();

    let sheets = [
        None,
        Some(Sheet::Menu),
        Some(Sheet::Panel(Tab::Homebrew)),
        Some(Sheet::Panel(Tab::Saves)),
        Some(Sheet::Panel(Tab::Media)),
        Some(Sheet::Panel(Tab::Info)),
    ];
    for sheet in sheets {
        app.mobile.sheet = sheet;
        small.extend(small_targets(&mut app, &ctx));
    }
    let first_game =
        crate::app::widgets::homebrew::details_id(crate::homebrew::HOMEBREW_GAMES[0].id);
    let mut details =
        egui::collapsing_header::CollapsingState::load_with_default_open(&ctx, first_game, true);
    details.set_open(true);
    details.store(&ctx);
    app.mobile.sheet = Some(Sheet::Panel(Tab::Homebrew));
    small.extend(small_targets(&mut app, &ctx));
    app.mobile.sheet = Some(Sheet::Panel(Tab::Settings));
    for tab in SettingsTab::iter_with_dev_mode(false) {
        app.ui.settings.current_tab = tab;
        small.extend(small_targets(&mut app, &ctx));
    }

    assert!(small.is_empty(), "too small to tap:\n{}", small.join("\n"));
}

fn playing(app: &mut Citrine) {
    app.emulator
        .load_rom(
            &Rom::new(&[0u8; 0x8000]),
            GbModel::Dmg,
            #[cfg(not(target_arch = "wasm32"))]
            None,
        )
        .expect("load");
    app.emulator.running = true;
}

#[test]
fn reset_asks_first_and_cancel_keeps_playing() {
    let mut app = phone_app("reset-cancel");
    playing(&mut app);
    app.run_hotkey(HotkeyAction::SoftReset);
    assert!(app.pending_reset.is_some(), "reset waits for an answer");
    assert!(!app.emulator.running, "the game holds still while asking");
    app.finish_reset(false);
    assert!(app.pending_reset.is_none());
    assert!(app.emulator.running);
    app.run_hotkey(HotkeyAction::SoftReset);
    assert!(
        app.pending_reset.is_some(),
        "a cancel doesn't stop the next ask"
    );
}

fn playing_both(app: &mut Citrine) -> String {
    let mut data = vec![0u8; 0x8000];
    data[0x0143] = 0x80;
    data[0x0147] = 0x03;
    let rom = Rom::new(&data);
    let model = app.ui.settings.console_for_rom(&rom);
    app.emulator.load_rom(&rom, model, None).expect("load");
    app.emulator.running = true;
    app.emulator.rom_key().expect("key").to_owned()
}

fn wait_until_playing(app: &mut Citrine) {
    std::thread::sleep(std::time::Duration::from_millis(5100));
    assert!(app.emulator.has_been_playing());
}

#[test]
fn a_fresh_game_switches_console_without_asking() {
    let mut app = phone_app("console-fresh");
    let key = playing_both(&mut app);
    assert_eq!(app.emulator.console(), GbModel::Cgb);
    app.events.play_on(GbModel::Dmg);
    app.handle_event_queue(true);
    assert!(app.pending_console.is_none());
    assert_eq!(app.emulator.console(), GbModel::Dmg);
    assert_eq!(
        app.ui.settings.console_for(&key, RomCgbMode::CgbAndGb),
        GbModel::Dmg,
        "the game remembers its console"
    );

    let mut again = phone_app("console-fresh-again");
    again.ui.settings = std::mem::take(&mut app.ui.settings);
    playing_both(&mut again);
    assert_eq!(
        again.emulator.console(),
        GbModel::Dmg,
        "and starts there next time"
    );
}

#[test]
fn switching_a_running_game_asks_first() {
    let mut app = phone_app("console-ask");
    let key = playing_both(&mut app);
    wait_until_playing(&mut app);
    app.events.play_on(GbModel::Dmg);
    app.handle_event_queue(true);
    assert!(
        app.pending_console.is_some(),
        "the switch waits for an answer"
    );
    assert!(!app.emulator.running, "the game holds still while asking");

    app.finish_play_on(false);
    assert!(app.emulator.running);
    assert_eq!(
        app.emulator.console(),
        GbModel::Cgb,
        "cancel keeps the console"
    );
    assert!(app.ui.settings.game_consoles.is_empty(), "and the choice");

    app.events.play_on(GbModel::Dmg);
    app.handle_event_queue(true);
    app.finish_play_on(true);
    assert_eq!(app.emulator.console(), GbModel::Dmg);
    assert_eq!(
        app.ui.settings.console_for(&key, RomCgbMode::CgbAndGb),
        GbModel::Dmg
    );
}

#[test]
fn a_game_for_one_console_offers_no_switch() {
    let mut app = phone_app("console-one");
    playing(&mut app);
    app.events.play_on(GbModel::Cgb);
    app.handle_event_queue(true);
    assert_eq!(app.emulator.console(), GbModel::Dmg);
    assert!(app.ui.settings.game_consoles.is_empty());
}

#[test]
fn dont_ask_again_lasts_until_citrine_closes() {
    let mut app = phone_app("reset-stop-asking");
    playing(&mut app);
    app.run_hotkey(HotkeyAction::SoftReset);
    app.pending_reset.as_mut().unwrap().stop_asking = true;
    app.finish_reset(true);
    app.run_hotkey(HotkeyAction::SoftReset);
    assert!(app.pending_reset.is_none(), "no second ask this session");
    assert!(app.emulator.running);

    let saved = serde_json::to_string(&app).unwrap();
    assert!(
        !saved.contains("without_asking") && !saved.contains("stop_asking"),
        "the choice is never saved"
    );
}

#[test]
fn opening_a_tab_in_the_touch_layout_opens_its_sheet() {
    let mut app = phone_app("open-tab");
    app.events.open_tab(Tab::Media);
    app.handle_event_queue(true);
    assert_eq!(app.mobile.sheet, Some(Sheet::Panel(Tab::Media)));
    assert!(
        app.dock.find_tab(&Tab::Media).is_none(),
        "no hidden dock tab"
    );

    let mut desktop = phone_app("open-tab-desktop");
    desktop.events.open_tab(Tab::Media);
    desktop.handle_event_queue(false);
    assert_eq!(desktop.mobile.sheet, None);
    assert!(desktop.dock.find_tab(&Tab::Media).is_some());
}

fn media_app(name: &str, layout: LayoutMode) -> Citrine {
    let mut app = phone_app(name);
    app.ui.settings.layout = layout;
    let root =
        std::env::temp_dir().join(format!("citrine-media-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    app.media = MediaStore::at(root);
    let mut rom = vec![0u8; 0x8000];
    rom[0x0147] = 0x03;
    app.emulator
        .load_rom(&citrine_gb::rom::Rom::new(&rom), GbModel::Dmg, None)
        .unwrap();
    app
}

fn media_in_front(app: &Citrine) -> bool {
    let Some((surface, node, tab)) = app.dock.find_tab(&Tab::Media) else {
        return false;
    };
    app.dock[surface][node]
        .get_leaf()
        .is_some_and(|leaf| leaf.active == tab)
}

fn game_in_view(app: &Citrine) -> bool {
    let (surface, node, tab) = app.dock.find_tab(&Tab::GameBoy).unwrap();
    app.dock[surface][node]
        .get_leaf()
        .is_some_and(|leaf| leaf.active == tab)
}

fn play_frame(app: &mut Citrine, ctx: &Context, events: Vec<egui::Event>) -> egui::FullOutput {
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
        mobile::touch_spacing(ctx, touch);
        app.render(ctx);
        app.show_toasts(ctx);
        app.handle_event_queue(touch);
    })
}

fn text_rect(output: &egui::FullOutput, wanted: &str) -> Option<egui::Rect> {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.text().trim() == wanted => {
                Some(text.visual_bounding_rect())
            }
            _ => None,
        })
}

fn load_game(app: &mut Citrine, rom: &[u8], save: Option<&[u8]>) {
    let rom = citrine_gb::rom::Rom::new(rom);
    if let Some(save) = save {
        let key = rom.header().unwrap().sha256_hex_string();
        app.emulator.store.store_battery(&key, save).unwrap();
    }
    app.emulator.load_rom(&rom, GbModel::Cgb, None).unwrap();
    app.watch_game_info();
}

fn crystal_app(name: &str, layout: LayoutMode, save: Option<&[u8]>) -> Citrine {
    let mut app = phone_app(name);
    app.ui.settings.layout = layout;
    load_game(
        &mut app,
        &citrine_gb_gamedata::fixtures::crystal_cartridge(),
        save,
    );
    app
}

fn text_containing(output: &egui::FullOutput, wanted: &str) -> Option<egui::Rect> {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.text().contains(wanted) => {
                Some(text.visual_bounding_rect())
            }
            _ => None,
        })
}

fn badge_count(output: &egui::FullOutput) -> usize {
    output
        .shapes
        .iter()
        .filter(|clipped| match &clipped.shape {
            egui::Shape::Path(path) => {
                let p = &path.points;
                p.len() == 4 && p[0].x == p[2].x && p[1].y == p[3].y && p[0].y < p[2].y
            }
            _ => false,
        })
        .count()
}

fn tap(at: egui::Pos2) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(at),
        egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        },
        egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        },
    ]
}

fn escape() -> Vec<egui::Event> {
    vec![egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }]
}

#[test]
fn game_info_is_offered_only_for_a_recognised_game() {
    let ctx = Context::default();
    let mut app = crystal_app("info-offered", LayoutMode::Mobile, None);
    assert!(app.game_info.recognised());
    app.mobile.open(Sheet::Menu);
    let output = play_frame(&mut app, &ctx, vec![]);
    assert!(text_containing(&output, Tab::GameInfo.label()).is_some());

    let ctx = Context::default();
    let mut other = media_app("info-not-offered", LayoutMode::Mobile);
    other.watch_game_info();
    assert!(!other.game_info.recognised());
    other.mobile.open(Sheet::Menu);
    let output = play_frame(&mut other, &ctx, vec![]);
    assert!(text_containing(&output, Tab::GameInfo.label()).is_none());
}

#[test]
fn game_info_closes_when_an_unsupported_game_loads() {
    let mut zero = vec![0u8; 0x8000];
    zero[0x0147] = 0x03;
    let latin = citrine_gb_gamedata::fixtures::japanese_cartridge_in_latin();
    for (name, rom) in [("zero", zero), ("latin", latin)] {
        let mut desktop = crystal_app(&format!("info-closes-{name}"), LayoutMode::Desktop, None);
        desktop.open_tab(Tab::GameInfo);
        assert!(desktop.dock.find_tab(&Tab::GameInfo).is_some());
        load_game(&mut desktop, &rom, None);
        assert!(!desktop.game_info.recognised(), "{name}");
        assert!(desktop.dock.find_tab(&Tab::GameInfo).is_none(), "{name}");

        let mut phone = crystal_app(&format!("info-sheet-{name}"), LayoutMode::Mobile, None);
        phone.mobile.open(Sheet::Panel(Tab::GameInfo));
        load_game(&mut phone, &rom, None);
        assert_eq!(phone.mobile.sheet, None, "{name}");
    }
}

#[test]
fn game_info_shows_eight_badges_in_gold_and_silver_until_kanto() {
    use citrine_gb_gamedata::fixtures::crystal_save_with_badges;
    for (reached, shown) in [(false, 8), (true, 16)] {
        let save = crystal_save_with_badges(0xFF, 0, reached);
        let ctx = Context::default();
        let mut app = crystal_app(
            &format!("info-badges-{reached}"),
            LayoutMode::Mobile,
            Some(&save),
        );
        app.mobile.open(Sheet::Panel(Tab::GameInfo));
        app.watch_game_info();
        let output = play_frame(&mut app, &ctx, vec![]);
        assert_eq!(badge_count(&output), shown, "reached Kanto: {reached}");
    }
}

#[test]
fn game_info_back_on_a_phone_closes_the_details_before_the_sheet() {
    let save = citrine_gb_gamedata::fixtures::crystal_save();
    let ctx = Context::default();
    let mut app = crystal_app("info-back", LayoutMode::Mobile, Some(&save));
    app.mobile.open(Sheet::Menu);
    app.mobile.open(Sheet::Panel(Tab::GameInfo));
    app.watch_game_info();
    app.game_info.view.details = true;
    assert!(app.game_info.details_open());

    let output = play_frame(&mut app, &ctx, vec![]);
    let back = text_containing(&output, "Party").expect("back to the party");
    play_frame(&mut app, &ctx, tap(back.center()));
    assert!(!app.game_info.details_open());
    assert_eq!(app.mobile.sheet, Some(Sheet::Panel(Tab::GameInfo)));

    app.game_info.view.details = true;
    let press = |app: &mut Citrine, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(412.0, 915.0),
            )),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            app.render(ctx);
            app.mobile_escape(ctx);
        });
    };
    press(&mut app, vec![]);
    press(&mut app, escape());
    assert!(!app.game_info.details_open());
    assert_eq!(app.mobile.sheet, Some(Sheet::Panel(Tab::GameInfo)));
    press(&mut app, escape());
    assert_eq!(app.mobile.sheet, Some(Sheet::Menu));
}

fn info_frame(app: &mut Citrine, ctx: &Context, width: f32) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width, 915.0),
        )),
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| {
        let touch = app.is_mobile(ctx);
        mobile::touch_spacing(ctx, touch);
        app.render(ctx);
    });
}

fn walk_game_info(app: &mut Citrine, ctx: &Context, width: f32) {
    use crate::game_info::Page;
    for page in [Page::Party, Page::Boxes, Page::Bag, Page::Pokedex] {
        app.game_info.view.page = page;
        app.game_info.view.details = false;
        info_frame(app, ctx, width);
    }
    app.game_info.view.page = Page::Party;
    for pick in 0..6 {
        app.game_info.view.party_pick = pick;
        app.game_info.view.details = true;
        info_frame(app, ctx, width);
    }
    app.game_info.view.page = Page::Boxes;
    for pc_box in 0..14 {
        app.game_info.view.pc_box = Some(pc_box);
        for pick in [0, 5, 19] {
            app.game_info.view.box_pick = pick;
            app.game_info.view.details = true;
            info_frame(app, ctx, width);
        }
    }
    app.game_info.view.page = Page::Pokedex;
    app.game_info.view.details = false;
    for (search, caught) in [("", true), ("a", false), ("zz", false)] {
        app.game_info.view.dex_search = search.to_owned();
        app.game_info.view.dex_caught = caught;
        info_frame(app, ctx, width);
    }
}

fn reach_overworld(app: &mut Citrine) -> bool {
    use citrine_gb::gb::joypad::JoypadState;
    for _ in 0..60 {
        app.emulator.gb.run_frame();
    }
    for step in 0..120 {
        let button = if step % 2 == 0 {
            JoypadState::START
        } else {
            JoypadState::A
        };
        app.emulator.gb.press_button(button);
        for _ in 0..6 {
            app.emulator.gb.run_frame();
        }
        app.emulator.gb.release_button(button);
        for _ in 0..54 {
            app.emulator.gb.run_frame();
        }
        app.game_info.next_read_now();
        app.watch_game_info();
        if app.game_info.game().is_some_and(|game| game.live.is_some()) {
            return true;
        }
    }
    false
}

#[test]
#[ignore]
fn game_info_opens_for_every_real_pokemon_game() {
    let home = std::path::PathBuf::from(std::env::var("HOME").unwrap());
    let dir = std::env::var_os("CITRINE_ROMS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home.join("roms/gb"));
    let silver_save = std::env::var_os("CITRINE_SILVER_SAVE").and_then(|p| std::fs::read(p).ok());
    for name in [
        "Pokemon - Red Version (USA, Europe).gb",
        "Pokemon Yellow.gb",
        "Pokemon - Gold Version (USA, Europe).gbc",
        "Pokemon - Silver Version (USA, Europe).gbc",
        "Pokemon Crystal.gbc",
    ] {
        let rom = std::fs::read(dir.join(name)).expect(name);
        let save = silver_save.as_deref().filter(|_| name.contains("Silver"));
        for (layout, width) in [(LayoutMode::Desktop, 1280.0), (LayoutMode::Mobile, 412.0)] {
            let ctx = Context::default();
            let mut app = phone_app(&format!("real-{}-{width}", name.len()));
            app.ui.settings.layout = layout;
            load_game(&mut app, &rom, save);
            assert!(app.game_info.recognised(), "{name}");
            if layout == LayoutMode::Desktop {
                app.open_tab(Tab::GameInfo);
            } else {
                app.mobile.open(Sheet::Panel(Tab::GameInfo));
            }
            app.watch_game_info();
            walk_game_info(&mut app, &ctx, width);
            let shown = app.game_info.game().and_then(|game| game.state()).is_some();
            println!("{name} at {width}: opened, save shown: {shown}");
            if save.is_some() {
                assert!(shown, "{name}");
                assert!(
                    reach_overworld(&mut app),
                    "{name} never reached the overworld"
                );
                walk_game_info(&mut app, &ctx, width);
                if layout == LayoutMode::Desktop {
                    time_game_info(&mut app);
                }
            }
        }
    }
}

fn time_game_info(app: &mut Citrine) {
    let rounds = 200;
    let start = web_time::Instant::now();
    for _ in 0..rounds {
        app.game_info.next_read_now();
        app.watch_game_info();
    }
    let read = start.elapsed() / rounds;
    let start = web_time::Instant::now();
    let idle_rounds = 100_000;
    for _ in 0..idle_rounds {
        app.watch_game_info();
    }
    let idle = start.elapsed() / idle_rounds;
    println!("a frame with a read: {read:?}; a frame without: {idle:?}");
}

#[test]
fn game_info_on_desktop_has_room_for_details_beside_the_list() {
    let save = citrine_gb_gamedata::fixtures::crystal_save();
    let ctx = Context::default();
    let mut app = crystal_app("info-room", LayoutMode::Desktop, Some(&save));
    app.open_tab(Tab::GameInfo);
    app.widen_for_game_info();
    app.watch_game_info();
    let _ = info_frame_out(&mut app, &ctx, 1280.0, 800.0);
    let output = info_frame_out(&mut app, &ctx, 1280.0, 800.0);
    let egg = text_rect(&output, "Egg").expect("party list");
    let stats = text_rect(&output, "Stats").expect("details");
    assert!(stats.left() > egg.right(), "details beside the list");
    let right = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) => Some(text.visual_bounding_rect().right()),
            _ => None,
        })
        .filter(|right| right.is_finite())
        .fold(0.0, f32::max);
    assert!(right <= 1280.0, "text reaches {right}");
}

fn info_frame_out(app: &mut Citrine, ctx: &Context, width: f32, height: f32) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width, height),
        )),
        ..Default::default()
    };
    ctx.run(input, |ctx| {
        let touch = app.is_mobile(ctx);
        mobile::touch_spacing(ctx, touch);
        app.render(ctx);
    })
}

#[test]
fn a_screenshot_brings_the_media_tab_to_front_on_desktop() {
    let mut app = media_app("shot-desktop", LayoutMode::Desktop);
    app.open_tab(Tab::Settings);
    assert!(!media_in_front(&app));

    app.take_screenshot();
    app.handle_event_queue(false);
    assert!(media_in_front(&app));
    assert!(game_in_view(&app));
    let first = app.ui.media.highlighted().cloned().expect("highlighted");

    let layout = format!("{:?}", app.dock.iter_all_tabs().collect::<Vec<_>>());
    app.take_screenshot();
    app.handle_event_queue(false);
    assert!(media_in_front(&app));
    assert_eq!(
        layout,
        format!("{:?}", app.dock.iter_all_tabs().collect::<Vec<_>>())
    );
    let second = app.ui.media.highlighted().cloned().expect("highlighted");
    assert_ne!(first, second);
    assert_eq!(app.mobile.sheet, None);
}

#[test]
fn a_screenshot_never_hides_the_game_behind_the_media_tab() {
    let mut app = media_app("shot-shared-leaf", LayoutMode::Desktop);
    app.dock = DockState::new(vec![Tab::GameBoy, Tab::Settings]);
    let (surface, node, tab) = app.dock.find_tab(&Tab::GameBoy).unwrap();
    app.dock.set_active_tab((surface, node, tab));

    app.take_screenshot();
    app.handle_event_queue(false);
    assert!(game_in_view(&app));
    assert!(app.dock.find_tab(&Tab::Media).is_none());
    assert_eq!(app.ui.media.highlighted(), None);
}

#[test]
fn a_phone_screenshot_keeps_the_game_and_offers_to_open_media() {
    let ctx = Context::default();
    let mut app = media_app("shot-phone", LayoutMode::Mobile);
    play_frame(&mut app, &ctx, Vec::new());
    app.take_screenshot();
    let mut output = play_frame(&mut app, &ctx, Vec::new());
    for _ in 0..10 {
        output = play_frame(&mut app, &ctx, Vec::new());
    }
    assert_eq!(app.mobile.sheet, None);
    assert!(text_rect(&output, "Screenshot saved to Media").is_some());
    let open = text_rect(&output, "Open").expect("an Open button on the toast");

    let at = open.center();
    play_frame(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    play_frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert_eq!(app.mobile.sheet, Some(Sheet::Panel(Tab::Media)));
    let id = app.media.items()[0].id.clone();
    assert_eq!(app.ui.media.highlighted(), Some(&id));
}

#[test]
fn a_forced_desktop_layout_falls_back_to_touch_on_a_narrow_window() {
    let phone = egui::vec2(412.0, 915.0);
    let landscape_phone = egui::vec2(915.0, 412.0);
    let desktop = egui::vec2(1280.0, 800.0);
    assert!(LayoutMode::Desktop.is_touch_at(phone));
    assert!(!LayoutMode::Desktop.is_touch_at(landscape_phone));
    assert!(!LayoutMode::Desktop.is_touch_at(desktop));
    assert!(LayoutMode::Auto.is_touch_at(landscape_phone));
    assert!(LayoutMode::Mobile.is_touch_at(desktop));
}

fn frame_at(
    app: &mut Citrine,
    ctx: &Context,
    css: egui::Vec2,
    pixel_ratio: f32,
) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            css / ctx.zoom_factor(),
        )),
        viewports: std::iter::once((
            egui::ViewportId::ROOT,
            egui::ViewportInfo {
                native_pixels_per_point: Some(pixel_ratio),
                ..Default::default()
            },
        ))
        .collect(),
        ..Default::default()
    };
    ctx.run(input, |ctx| {
        let touch = app.is_mobile(ctx);
        mobile::touch_spacing(ctx, touch);
        app.render(ctx);
        app.ui
            .settings
            .apply(ctx, &mut app.audio, &mut app.emulator, touch);
    })
}

fn settle(app: &mut Citrine, ctx: &Context, css: egui::Vec2, pixel_ratio: f32) -> egui::FullOutput {
    for _ in 0..3 {
        frame_at(app, ctx, css, pixel_ratio);
    }
    frame_at(app, ctx, css, pixel_ratio)
}

fn touch_heights_in_css_px(
    ctx: &Context,
    output: egui::FullOutput,
    pixel_ratio: f32,
) -> (Vec<egui::Rect>, Vec<f32>) {
    let labels: Vec<egui::Pos2> = output
        .shapes
        .into_iter()
        .filter_map(|clipped| match clipped.shape {
            egui::Shape::Text(text)
                if ["Load a ROM", "Homebrew games"]
                    .iter()
                    .any(|label| text.galley.text().ends_with(label)) =>
            {
                Some(text.visual_bounding_rect().center())
            }
            _ => None,
        })
        .collect();
    let css_per_point = ctx.pixels_per_point() / pixel_ratio;
    let bar = egui::containers::panel::PanelState::load(ctx, egui::Id::new("mobile_bar"))
        .expect("the phone bar is drawn")
        .rect;
    ctx.viewport(|viewport| {
        let clickable: Vec<egui::Rect> = viewport
            .prev_pass
            .widgets
            .layers()
            .flat_map(|(_, widgets)| widgets.iter())
            .filter(|widget| widget.sense.senses_click() && widget.sense.is_focusable())
            .filter(|widget| widget.rect.area() > 0.0)
            .map(|widget| widget.rect)
            .collect();
        let bar_buttons = clickable
            .iter()
            .filter(|rect| bar.contains_rect(**rect))
            .map(|rect| *rect * css_per_point)
            .collect();
        let empty_state = labels
            .iter()
            .filter_map(|label| {
                clickable
                    .iter()
                    .filter(|rect| rect.contains(*label))
                    .map(|rect| rect.height() * css_per_point)
                    .reduce(f32::min)
            })
            .collect();
        (bar_buttons, empty_state)
    })
}

fn assert_touch_sized(ctx: &Context, output: egui::FullOutput, pixel_ratio: f32, scale: f32) {
    let (bar, empty_state) = touch_heights_in_css_px(ctx, output, pixel_ratio);
    let at = format!("pixel ratio {pixel_ratio}, ui scale {scale}");
    assert!(bar.len() >= 3, "{at}: {} phone bar buttons", bar.len());
    assert_eq!(empty_state.len(), 2, "{at}: Load a ROM and Homebrew games");
    let tallest_icon = if scale > 1.5 { 48.0 } else { TOUCH_TARGET_PX };
    for rect in &bar {
        assert!(
            (TOUCH_TARGET_PX - 0.5..tallest_icon + 0.5).contains(&rect.height()),
            "{at}: phone bar button {:.1} css px tall",
            rect.height()
        );
    }
    for (i, a) in bar.iter().enumerate() {
        for b in &bar[i + 1..] {
            assert!(
                a.intersect(*b).area() < 1.0,
                "{at}: phone bar buttons {a:?} and {b:?} overlap"
            );
        }
    }
    for height in empty_state {
        assert!(
            (height - TOUCH_TARGET_PX).abs() < 0.5,
            "{at}: empty state button {height:.1} css px tall"
        );
    }
}

#[test]
fn touch_controls_are_44_css_px_at_every_pixel_ratio_and_ui_scale() {
    let phone = egui::vec2(412.0, 915.0);
    for pixel_ratio in [1.0, 2.0, 2.625, 3.0, 3.5] {
        for scale in [0.6, 1.0, 1.5, 2.0] {
            let ctx = Context::default();
            let mut app = phone_app(&format!("dpr-{pixel_ratio}-{scale}"));
            app.ui.settings.touch.ui_scale = scale;
            app.ui.settings.ui_scale = scale;
            app.ui.settings.touch.show_controls = false;
            let output = settle(&mut app, &ctx, phone, pixel_ratio);
            let wanted = TOUCH_TARGET_PX * pixel_ratio / ctx.pixels_per_point();
            assert!(
                (ctx.style().spacing.interact_size.y - wanted).abs() < 0.01,
                "pixel ratio {pixel_ratio}, ui scale {scale}: interact size {} points, wanted {wanted}",
                ctx.style().spacing.interact_size.y
            );
            assert_touch_sized(&ctx, output, pixel_ratio, scale);
        }
    }
}

#[test]
fn a_new_pixel_ratio_or_ui_scale_resizes_touch_controls_without_a_restart() {
    let phone = egui::vec2(412.0, 915.0);
    let ctx = Context::default();
    let mut app = phone_app("dpr-change");
    app.ui.settings.touch.show_controls = false;
    let output = settle(&mut app, &ctx, phone, 1.0);
    assert_touch_sized(&ctx, output, 1.0, 1.0);
    let output = settle(&mut app, &ctx, phone, 3.0);
    assert_touch_sized(&ctx, output, 3.0, 1.0);
    app.ui.settings.touch.ui_scale = 1.5;
    let output = settle(&mut app, &ctx, phone / 1.25, 3.75);
    assert_touch_sized(&ctx, output, 3.75, 1.5);
}

fn desk_frame(app: &mut Citrine, ctx: &Context, events: Vec<egui::Event>) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 800.0),
        )),
        events,
        ..Default::default()
    };
    ctx.run(input, |ctx| {
        app.render(ctx);
        app.handle_event_queue(false);
        app.ui
            .settings
            .apply(ctx, &mut app.audio, &mut app.emulator, false);
    })
}

fn click_at(app: &mut Citrine, ctx: &Context, at: egui::Pos2) -> egui::FullOutput {
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    desk_frame(app, ctx, vec![egui::Event::PointerMoved(at), press(true)]);
    desk_frame(app, ctx, vec![press(false)])
}

#[test]
fn a_channel_muted_in_the_apu_tab_stays_muted_after_a_volume_change() {
    let ctx = Context::default();
    let mut app = media_app("apu-mute", LayoutMode::Desktop);
    app.open_tab(Tab::Apu);
    let mut output = desk_frame(&mut app, &ctx, Vec::new());
    for _ in 0..3 {
        output = desk_frame(&mut app, &ctx, Vec::new());
    }
    let second = text_rect(&output, "2").expect("the APU tab's channel 2 box");

    click_at(&mut app, &ctx, second.center());
    desk_frame(&mut app, &ctx, Vec::new());
    assert!(app.emulator.gb.debugger.ch2_disabled);

    app.ui.settings.volume = 0.5;
    app.ui.settings.dirty = true;
    desk_frame(&mut app, &ctx, Vec::new());

    assert!(app.emulator.gb.debugger.ch2_disabled);
    assert_eq!(app.ui.settings.channels, [true, false, true, true]);
}

#[test]
fn a_controller_button_bound_to_focus_mode_turns_it_on_and_off() {
    let mut app = Citrine::default();
    app.ui
        .settings
        .hotkeys
        .bind_pad(PadButton::Mode, HotkeyAction::ToggleFocusMode);
    app.run_pad_shortcuts(vec![PadButton::Mode]);
    assert!(app.ui.settings.focus_mode);
    assert!(
        app.focus_exit_names()
            .contains(&PadButton::Mode.to_string())
    );
    app.run_pad_shortcuts(vec![PadButton::Mode]);
    assert!(!app.ui.settings.focus_mode);
}

#[test]
fn delete_everything_leaves_the_app_as_a_first_launch_does() {
    let mut app = media_app("delete-everything", LayoutMode::Desktop);
    app.emulator.save_snapshot(0).unwrap();
    let key = app.emulator.rom_key().unwrap().to_owned();
    app.emulator.store.store_battery(&key, b"battery").unwrap();
    let png = crate::utils::screenshot::encode_png(160, 144, &vec![9; 160 * 144 * 4]).unwrap();
    app.media
        .add_screenshot(
            crate::media::NewMedia::current(crate::media::MediaKind::Screenshot, &app.emulator),
            &png,
        )
        .unwrap();
    app.ui.settings.volume = 0.2;
    app.ui.settings.matrix = true;
    app.ui.settings.online.joined = Some("relay.example".into());
    app.ui.settings.current_tab = SettingsTab::YourData;
    app.ui
        .recent
        .record(std::path::Path::new("/roms/game.gb"), "Game", None);
    app.open_tab(Tab::Settings);
    app.mobile.open(Sheet::Panel(Tab::Settings));
    assert!(!app.emulator.store.files().is_empty());

    app.delete_everything();

    let fresh = Citrine::default();
    let tabs = |app: &Citrine| {
        app.dock
            .iter_all_tabs()
            .map(|(_, tab)| *tab)
            .collect::<Vec<_>>()
    };
    assert_eq!(tabs(&app), tabs(&fresh), "the first-launch tabs");
    assert_eq!(app.mobile.sheet, None);
    assert!(app.emulator.rom_key().is_none(), "no game is loaded");
    assert!(app.emulator.store.files().is_empty(), "no saves");
    assert!(app.media.items().is_empty(), "no media");
    assert!(app.ui.recent.is_empty(), "no recent games");
    let without_account = |settings: &Settings| {
        let mut value = serde_json::to_value(settings).unwrap();
        value.as_object_mut().unwrap().remove("online");
        value
    };
    assert_eq!(
        without_account(&app.ui.settings),
        without_account(&fresh.ui.settings),
        "default settings"
    );
    assert_eq!(
        app.ui.settings.online.joined.as_deref(),
        Some("relay.example"),
        "the online account stays"
    );
}

fn partner_frame(app: &mut Citrine, ctx: &Context, events: Vec<egui::Event>) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 800.0),
        )),
        events,
        ..Default::default()
    };
    ctx.run(input, |ctx| {
        let touch = app.is_mobile(ctx);
        mobile::touch_spacing(ctx, touch);
        app.render(ctx);
        app.handle_event_queue(touch);
    })
}

fn partner_cartridge() -> Vec<u8> {
    let mut rom = citrine_gb_gamedata::fixtures::crystal_cartridge();
    let moves: Vec<u8> = (1..=251u8)
        .flat_map(|id| {
            let pp = match id {
                1 => 35,
                2 => 25,
                3 => 10,
                _ => 20,
            };
            [id, 0, 40, 0, 100, pp, 0]
        })
        .collect();
    citrine_gb_gamedata::fixtures::put(&mut rom, 0x100000, &moves);
    let base: Vec<u8> = (1..=251u8)
        .flat_map(|dex| {
            let mut record = [50u8; 32];
            record[0] = dex;
            record[13] = 0x1F;
            record[22] = 0;
            record
        })
        .collect();
    citrine_gb_gamedata::fixtures::put(&mut rom, 0x110000, &base);
    rom
}

#[test]
fn a_picked_event_partner_goes_back_to_the_event_list() {
    let ctx = Context::default();
    let mut app = phone_app("partner-back");
    app.ui.settings.layout = LayoutMode::Desktop;
    let rom = partner_cartridge();
    load_game(&mut app, &rom, None);
    let game = citrine_gb_gamedata::identify(&rom).expect("the fixture is a known game");
    let event = citrine_gb_partner::events_for(game)
        .find(|event| event.generation == citrine_gb_gamedata::Generation::Two && !event.egg)
        .expect("a Gold and Silver event");
    app.open_tab(Tab::Playing);
    app.ui.linked.listing = true;
    app.start_partner(event);
    assert!(app.emulator.partner.is_some(), "the partner started");
    let _ = partner_frame(&mut app, &ctx, vec![]);
    let output = partner_frame(&mut app, &ctx, vec![]);
    let back = text_containing(&output, "All events").expect("a way back to the list");
    let _ = partner_frame(&mut app, &ctx, tap(back.center()));
    assert!(app.ui.linked.listing, "the list is open again");

    let _ = partner_frame(&mut app, &ctx, escape());
    assert!(
        !app.ui.linked.listing,
        "Esc returns to the partner it came from"
    );
    let _ = partner_frame(&mut app, &ctx, escape());
    let _ = partner_frame(&mut app, &ctx, vec![]);
    assert!(
        !app.ui.linked.listing,
        "Esc stops on the partner opened from the list"
    );

    app.ui.linked.listing = true;
    app.start_partner(event);
    let _ = partner_frame(&mut app, &ctx, vec![]);
    let _ = partner_frame(&mut app, &ctx, escape());
    assert!(
        app.ui.linked.listing,
        "Esc after a pick goes back to the list"
    );
}

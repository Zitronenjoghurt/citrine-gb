use super::*;
use crate::app::ui_state::hotkeys::HotkeyAction;
use crate::app::ui_state::keybinds::PadButton;
use crate::app::ui_state::settings::LayoutMode;
use crate::app::ui_state::settings::SettingsTab;
use citrine_gb::rom::Rom;
use citrine_gb::rom::header::RomCgbMode;
use mobile::TOUCH_TARGET_PX;
use mobile::nav::Place;

mod chat_keys;
#[cfg(feature = "gba")]
mod gba_panels;
mod restart;
mod rotation;

pub(crate) fn phone_app(name: &str) -> Citrine {
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
                        nav_name(app),
                        widget.rect.height(),
                        label
                    )
                })
            })
            .collect()
    })
}

fn nav_name(app: &Citrine) -> String {
    let nav = &app.mobile.nav;
    format!(
        "{:?}/{:?}/{:?}/more {}",
        nav.place(),
        nav.page(),
        nav.settings_page(),
        nav.more_open()
    )
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

    app.mobile.nav.show_game();
    small.extend(small_targets(&mut app, &ctx));
    app.mobile.nav.set_more(true);
    small.extend(small_targets(&mut app, &ctx));
    for place in Place::ALL {
        app.mobile.nav = Default::default();
        app.mobile.nav.go(place);
        small.extend(small_targets(&mut app, &ctx));
    }
    for tab in [Tab::Homebrew, Tab::Saves, Tab::Info, Tab::Stats] {
        app.mobile.nav = Default::default();
        app.mobile.open(tab);
        small.extend(small_targets(&mut app, &ctx));
    }
    let first_game =
        crate::app::widgets::homebrew::details_id(crate::homebrew::HOMEBREW_GAMES[0].id);
    let mut details =
        egui::collapsing_header::CollapsingState::load_with_default_open(&ctx, first_game, true);
    details.set_open(true);
    details.store(&ctx);
    app.mobile.nav = Default::default();
    app.mobile.open(Tab::Homebrew);
    small.extend(small_targets(&mut app, &ctx));
    for tab in SettingsTab::iter_with_dev_mode(false) {
        app.mobile.nav.open_settings(tab);
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
fn opening_a_tab_in_the_touch_layout_goes_to_its_place() {
    let mut app = phone_app("open-tab");
    app.events.open_tab(Tab::Media);
    app.handle_event_queue(true);
    assert_eq!(app.mobile.nav.place(), Place::Media);
    assert!(app.mobile.showing(Tab::Media));
    assert!(
        app.dock.find_tab(&Tab::Media).is_none(),
        "no hidden dock tab"
    );

    let mut desktop = phone_app("open-tab-desktop");
    desktop.events.open_tab(Tab::Media);
    desktop.handle_event_queue(false);
    assert!(!desktop.mobile.showing(Tab::Media));
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

pub(crate) fn play_frame(
    app: &mut Citrine,
    ctx: &Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
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

#[cfg(feature = "gba")]
pub(crate) fn inside_its_clip(output: &egui::FullOutput, wanted: &str) -> bool {
    output.shapes.iter().any(|clipped| match &clipped.shape {
        egui::Shape::Text(text) if text.galley.text().trim() == wanted => {
            text.visual_bounding_rect().max.x <= clipped.clip_rect.max.x + 0.5
        }
        _ => false,
    })
}

pub(crate) fn text_rect(output: &egui::FullOutput, wanted: &str) -> Option<egui::Rect> {
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

pub(crate) fn tap(at: egui::Pos2) -> Vec<egui::Event> {
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
    app.mobile.nav.show_game();
    app.mobile.nav.set_more(true);
    play_frame(&mut app, &ctx, vec![]);
    let output = play_frame(&mut app, &ctx, vec![]);
    assert!(text_containing(&output, Tab::GameInfo.label()).is_some());

    let ctx = Context::default();
    let mut other = media_app("info-not-offered", LayoutMode::Mobile);
    other.watch_game_info();
    assert!(!other.game_info.recognised());
    other.mobile.nav.show_game();
    other.mobile.nav.set_more(true);
    play_frame(&mut other, &ctx, vec![]);
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
        phone.mobile.open(Tab::GameInfo);
        load_game(&mut phone, &rom, None);
        assert!(!phone.mobile.showing(Tab::GameInfo), "{name}");
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
        app.mobile.open(Tab::GameInfo);
        app.watch_game_info();
        let output = play_frame(&mut app, &ctx, vec![]);
        assert_eq!(badge_count(&output), shown, "reached Kanto: {reached}");
    }
}

#[test]
fn game_info_back_on_a_phone_closes_the_details_before_the_page() {
    let save = citrine_gb_gamedata::fixtures::crystal_save();
    let ctx = Context::default();
    let mut app = crystal_app("info-back", LayoutMode::Mobile, Some(&save));
    app.mobile.nav.show_game();
    app.mobile.open(Tab::GameInfo);
    app.watch_game_info();
    app.game_info.view.details = true;
    assert!(app.game_info.details_open());

    let output = play_frame(&mut app, &ctx, vec![]);
    assert!(
        text_containing(&output, "Party").is_some(),
        "the details name their page"
    );
    let back = text_containing(&output, crate::icons::ARROW_LEFT).expect("a back arrow");
    play_frame(&mut app, &ctx, tap(back.center()));
    assert!(!app.game_info.details_open());
    assert!(app.mobile.showing(Tab::GameInfo));

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
    assert!(app.mobile.showing(Tab::GameInfo));
    press(&mut app, escape());
    assert!(app.mobile.nav.on_game());
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
    use citrine_gb::console::Keys;
    for _ in 0..60 {
        app.emulator.gb_mut().run_frame();
    }
    for step in 0..120 {
        let button = if step % 2 == 0 { Keys::START } else { Keys::A };
        app.emulator.gb_mut().press_button(button.into());
        for _ in 0..6 {
            app.emulator.gb_mut().run_frame();
        }
        app.emulator.gb_mut().release_button(button.into());
        for _ in 0..54 {
            app.emulator.gb_mut().run_frame();
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
                app.mobile.open(Tab::GameInfo);
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

#[test]
fn game_info_pages_fit_the_panel_at_every_desktop_width() {
    use crate::game_info::Page;
    let save = citrine_gb_gamedata::fixtures::crystal_save();
    for width in [560.0, 600.0, 640.0, 670.0, 720.0, 800.0, 1000.0, 1280.0] {
        for page in [Page::Map, Page::Party, Page::Journey] {
            let ctx = Context::default();
            let mut app = crystal_app("info-fit", LayoutMode::Desktop, Some(&save));
            app.open_tab(Tab::GameInfo);
            app.widen_for_game_info();
            app.watch_game_info();
            app.game_info.view.page = page;
            let _ = info_frame_out(&mut app, &ctx, width, 800.0);
            let output = info_frame_out(&mut app, &ctx, width, 800.0);
            let status = output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text)
                        if text.galley.text().starts_with("Save file")
                            || text.galley.text().starts_with("Playing") =>
                    {
                        Some(text.visual_bounding_rect())
                    }
                    _ => None,
                })
                .expect("status line");
            let left = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) => Some(text.visual_bounding_rect()),
                    _ => None,
                })
                .filter(|rect| (rect.center().y - status.center().y).abs() < 4.0)
                .map(|rect| rect.left())
                .fold(f32::INFINITY, f32::min);
            for clipped in &output.shapes {
                let rect = match &clipped.shape {
                    egui::Shape::Text(text) => text.visual_bounding_rect(),
                    egui::Shape::Rect(rect) => rect.rect,
                    _ => continue,
                };
                if rect.top() > status.top()
                    && rect.left() >= left - 0.5
                    && rect.left() < status.right()
                    && rect.right().is_finite()
                {
                    assert!(
                        rect.right() <= status.right() + 1.0,
                        "a shape on {page:?} ends at {}, the panel at {} (screen {width})",
                        rect.right(),
                        status.right()
                    );
                }
            }
        }
    }
}

pub(crate) fn info_frame_out(
    app: &mut Citrine,
    ctx: &Context,
    width: f32,
    height: f32,
) -> egui::FullOutput {
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
    assert!(!app.mobile.showing(Tab::Media));
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
    assert!(!app.mobile.showing(Tab::Media));
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
    assert!(app.mobile.showing(Tab::Media));
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
                if ["Open a ROM", "Homebrew games"]
                    .iter()
                    .any(|label| text.galley.text().ends_with(label)) =>
            {
                Some(text.visual_bounding_rect().center())
            }
            _ => None,
        })
        .collect();
    let css_per_point = ctx.pixels_per_point() / pixel_ratio;
    let bar = egui::containers::panel::PanelState::load(ctx, egui::Id::new("place_bar"))
        .expect("the phone bar is drawn")
        .rect
        .expand(1.0 / ctx.pixels_per_point());
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
    assert_eq!(empty_state.len(), 2, "{at}: Open a ROM and Homebrew games");
    for rect in &bar {
        assert!(
            rect.height() >= TOUCH_TARGET_PX - 0.5,
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
            height >= TOUCH_TARGET_PX - 0.5,
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
    let second = text_rect(&output, "Pulse 2").expect("the APU tab's Pulse 2 box");

    click_at(&mut app, &ctx, second.center());
    desk_frame(&mut app, &ctx, Vec::new());
    assert!(!app.emulator.gb().apu.channel_enabled(1));

    app.ui.settings.volume = 0.5;
    app.ui.settings.dirty = true;
    desk_frame(&mut app, &ctx, Vec::new());

    assert!(!app.emulator.gb().apu.channel_enabled(1));
    assert_eq!(app.ui.settings.channels, [true, false, true, true]);
}

#[test]
fn a_channel_muted_in_settings_stays_muted_after_loading_a_snapshot() {
    let ctx = Context::default();
    let mut app = media_app("mute-snapshot", LayoutMode::Desktop);
    app.ui.settings.channels = [true, true, false, true];
    app.ui.settings.dirty = true;
    desk_frame(&mut app, &ctx, Vec::new());
    app.emulator.save_snapshot(0).unwrap();

    assert!(app.emulator.load_snapshot(0).unwrap());
    desk_frame(&mut app, &ctx, Vec::new());

    assert!(!app.emulator.gb().apu.channel_enabled(2));
    assert!(app.emulator.gb().apu.channel_enabled(0));
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
    app.mobile.nav.open_settings(SettingsTab::YourData);
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
    assert_eq!(app.mobile.nav, mobile::nav::Nav::default());
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

#[derive(Default)]
struct MemoryStorage(std::collections::HashMap<String, String>);

impl eframe::Storage for MemoryStorage {
    fn get_string(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }

    fn set_string(&mut self, key: &str, value: String) {
        self.0.insert(key.to_owned(), value);
    }

    fn flush(&mut self) {}
}

fn stored_with(tabs: Vec<Tab>) -> Vec<Tab> {
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            let mut saved = phone_app("stored-tabs");
            saved.dock = DockState::new(tabs);
            let mut storage = MemoryStorage::default();
            eframe::set_value(&mut storage, eframe::APP_KEY, &saved);
            drop(saved);
            let app = Citrine::restore(Some(&storage));
            app.dock.iter_all_tabs().map(|(_, tab)| *tab).collect()
        })
        .expect("thread")
        .join()
        .expect("restored")
}

#[test]
fn a_layout_saved_with_apu_waves_opens_the_apu_tab() {
    let tabs = stored_with(vec![Tab::GameBoy, Tab::ApuWaves, Tab::Settings]);
    assert_eq!(tabs, [Tab::GameBoy, Tab::Apu, Tab::Settings]);
}

#[test]
fn a_layout_saved_with_both_apu_tabs_keeps_one() {
    let tabs = stored_with(vec![Tab::GameBoy, Tab::Apu, Tab::ApuWaves]);
    assert_eq!(tabs, [Tab::GameBoy, Tab::Apu]);
}

#[test]
fn a_gba_game_picked_as_the_second_game_says_what_it_is() {
    let mut app = phone_app("gba-link");
    let mut data = vec![0; 0x200];
    data[0..4].copy_from_slice(&[0x2E, 0x00, 0x00, 0xEA]);
    data[0xB2] = 0x96;
    app.ui.linked.picking = true;
    app.link_picked(crate::utils::file_loader::PickedFile {
        name: "Test.gba".to_string(),
        data,
        path: None,
    });
    assert_eq!(
        app.toasts.texts(),
        ["Game Boy Advance games link only with each other"]
    );
    assert!(app.emulator.link.is_none());
}

#[cfg(feature = "gba")]
#[test]
fn a_gba_game_picked_to_play_online_goes_to_the_invite() {
    let mut app = phone_app("gba-online");
    let mut data = vec![0; 0x200];
    data[0..4].copy_from_slice(&[0x2E, 0x00, 0x00, 0xEA]);
    data[0xB2] = 0x96;
    let friend = citrine_gb_net::OnlineId::new(7).expect("id");
    app.picked_for(super::tabs::playing::PickFor::Invite(friend), |app| {
        app.handle_load_rom_data(data)
    });
    assert_eq!(app.emulator.kind(), citrine_gb::console::ConsoleKind::Gba);
    assert_eq!(app.toasts.texts(), ["Couldn't send the invite"]);
    let card = app
        .emulator
        .game_file()
        .and_then(|file| file.card())
        .unwrap();
    assert_eq!(card.console, citrine_gb_net::Console::Gba);
}

#[test]
fn the_more_sheet_shows_close_game_on_phone_screens() {
    for (css, pixel_ratio) in [
        (egui::vec2(412.0, 915.0), 2.625),
        (egui::vec2(360.0, 800.0), 3.0),
    ] {
        let ctx = Context::default();
        let mut app = phone_app(&format!("more-{}", css.x));
        let mut rom = vec![0u8; 0x8000];
        rom[0x0147] = 0x03;
        rom[0x0149] = 0x02;
        app.emulator
            .load_rom(&citrine_gb::rom::Rom::new(&rom), GbModel::Dmg, None)
            .unwrap();
        app.emulator.save_snapshot(0).unwrap();
        app.ui.snapshots.invalidate();
        app.mobile.nav.show_game();
        app.mobile.nav.set_more(true);
        let output = settle(&mut app, &ctx, css, pixel_ratio);
        let bottom = ctx.content_rect().bottom();
        let close = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text() == "Close game" => {
                    let rect = text.visual_bounding_rect();
                    Some((rect.intersect(clipped.clip_rect).height(), rect.bottom()))
                }
                _ => None,
            })
            .expect("the sheet draws Close game");
        assert!(
            close.0 > 0.0 && close.1 <= bottom,
            "{css:?}: Close game ends at {} under a screen bottom of {bottom}",
            close.1
        );
    }
}

#[test]
fn a_layout_saved_with_time_control_opens_the_debugger_in_its_place() {
    let tabs = stored_with(vec![Tab::GameBoy, Tab::TimeControl, Tab::Registers]);
    assert_eq!(tabs, [Tab::GameBoy, Tab::Disassembly, Tab::Registers]);
}

#[test]
fn a_layout_saved_with_both_old_debugger_tabs_keeps_one() {
    let tabs = stored_with(vec![Tab::GameBoy, Tab::Disassembly, Tab::TimeControl]);
    assert_eq!(tabs, [Tab::GameBoy, Tab::Disassembly]);
}

fn debugger_text_positions(css: egui::Vec2, beside_game: bool) -> (egui::Pos2, egui::Pos2) {
    let ctx = Context::default();
    let mut app = phone_app(&format!("debugger-{}", css.x));
    app.ui.settings.layout = LayoutMode::Desktop;
    load_tobu(&mut app);
    app.dock = DockState::new(vec![Tab::Disassembly]);
    if beside_game {
        app.dock = DockState::new(vec![Tab::GameBoy]);
        app.dock.main_surface_mut().split_right(
            egui_dock::NodeIndex::root(),
            0.6,
            vec![Tab::Disassembly],
        );
    }
    let output = settle(&mut app, &ctx, css, 1.0);
    let find = |wanted: &dyn Fn(&str) -> bool| {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if wanted(text.galley.text()) => Some(text.pos),
                _ => None,
            })
            .unwrap_or_else(|| panic!("text is drawn at {css:?}"))
    };
    (find(&is_code_address), find(&|text| text == "CPU"))
}

fn is_code_address(text: &str) -> bool {
    text.len() == 7
        && text.as_bytes()[2] == b':'
        && text
            .chars()
            .filter(|c| *c != ':')
            .all(|c| c.is_ascii_hexdigit())
}

fn load_tobu(app: &mut Citrine) {
    let tobu = include_bytes!("../../homebrew/tobu.gb");
    app.emulator
        .load_rom(&Rom::new(tobu), citrine_gb::gb::GbModel::Dmg, None)
        .expect("tobu loads");
    app.emulator.gb_mut().debugger.static_analysis_enabled = true;
    for _ in 0..30 {
        app.emulator.gb_mut().run_frame();
    }
}

#[test]
fn a_debugger_beside_the_game_puts_the_cpu_beside_the_code() {
    let (code, cpu) = debugger_text_positions(egui::vec2(760.0, 800.0), false);
    assert!(cpu.x > code.x + 200.0, "code at {code:?}, CPU at {cpu:?}");
}

#[test]
fn a_debugger_in_the_side_dock_puts_the_code_above_the_cpu() {
    let (code, cpu) = debugger_text_positions(egui::vec2(1280.0, 800.0), true);
    assert!(code.y < cpu.y, "code at {code:?}, CPU at {cpu:?}");
}

fn docked_code_rows(ui_scale: f32, cpu_open: bool) -> (usize, String) {
    let ctx = Context::default();
    let mut app = phone_app(&format!("code-rows-{ui_scale}-{cpu_open}"));
    app.ui.settings.layout = LayoutMode::Desktop;
    app.ui.settings.ui_scale = ui_scale;
    app.ui.registers.docked_cpu = cpu_open;
    load_tobu(&mut app);
    app.open_tab(Tab::Disassembly);
    let output = settle(&mut app, &ctx, egui::vec2(1280.0, 800.0), 1.0);
    let texts: Vec<(String, egui::Rect, egui::Rect)> = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) => Some((
                text.galley.text().to_owned(),
                text.visual_bounding_rect(),
                clipped.clip_rect,
            )),
            _ => None,
        })
        .collect();
    let rows: Vec<&(String, egui::Rect, egui::Rect)> = texts
        .iter()
        .filter(|(text, rect, clip)| is_code_address(text) && clip.contains_rect(*rect))
        .collect();
    let layout = format!(
        "screen {:?}, rows {:?}, texts {:?}",
        ctx.content_rect(),
        rows.iter()
            .map(|(_, rect, _)| rect.top())
            .collect::<Vec<_>>(),
        texts
            .iter()
            .filter(|(text, rect, _)| rect.left() > 330.0 && !is_code_address(text))
            .map(|(text, rect, _)| (text.chars().take(12).collect::<String>(), rect.top()))
            .collect::<Vec<_>>()
    );
    (rows.len(), layout)
}

#[test]
fn the_docked_debugger_shows_ten_code_rows_at_web_zoom() {
    for (ui_scale, cpu_open) in [(1.5, false), (1.5, true), (2.2, false)] {
        let (rows, layout) = docked_code_rows(ui_scale, cpu_open);
        assert!(
            rows >= 10,
            "{rows} rows at {ui_scale}, CPU open {cpu_open}: {layout}"
        );
    }
}

const DOCK_BEFORE_DEV_TOOLS: &str = include_str!("../../fixtures/dock-v0.11.0.ron");

fn load_dock(text: &str) -> Option<DockState<Tab>> {
    let mut storage = MemoryStorage::default();
    eframe::Storage::set_string(&mut storage, "dock", text.trim().to_owned());
    eframe::get_value(&storage, "dock")
}

#[test]
fn a_dock_layout_saved_before_the_dev_tools_menu_still_loads() {
    let mut dock = load_dock(DOCK_BEFORE_DEV_TOOLS).expect("loads");
    tabs::retire_old_tabs(&mut dock);
    let tabs: Vec<Tab> = dock.iter_all_tabs().map(|(_, tab)| *tab).collect();
    assert!(tabs.contains(&Tab::Disassembly));
    assert!(tabs.contains(&Tab::AudioDebug));
    assert!(!tabs.contains(&Tab::TimeControl));
    assert_eq!(tabs.len(), 19);
}

#[test]
fn a_tool_from_a_newer_build_loads_and_closes() {
    let newer = DOCK_BEFORE_DEV_TOOLS.replace("Performance", "Tool(Hologram)");
    let mut dock = load_dock(&newer).expect("loads");
    assert!(dock.find_tab(&Tab::Tool(tabs::Tool::Unknown)).is_some());
    tabs::retire_old_tabs(&mut dock);
    assert!(dock.find_tab(&Tab::Tool(tabs::Tool::Unknown)).is_none());
    assert!(dock.find_tab(&Tab::Settings).is_some());
}

fn settings_page(name: &str, page: SettingsTab) -> (Citrine, Context) {
    let ctx = Context::default();
    let mut app = phone_app(name);
    app.mobile.nav.go(Place::Settings);
    app.mobile.nav.open_settings(page);
    play_frame(&mut app, &ctx, vec![]);
    (app, ctx)
}

fn last_text_rect(output: &egui::FullOutput, wanted: &str) -> Option<egui::Rect> {
    output
        .shapes
        .iter()
        .rev()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.text().trim() == wanted => {
                Some(text.visual_bounding_rect())
            }
            _ => None,
        })
}

#[test]
fn back_closes_a_settings_sheet_before_the_page() {
    let (mut app, ctx) = settings_page("sheet-back", SettingsTab::Sound);
    let output = play_frame(&mut app, &ctx, vec![]);
    let volume = text_rect(&output, "Volume").expect("a volume row");
    play_frame(&mut app, &ctx, tap(volume.center()));
    play_frame(&mut app, &ctx, vec![]);
    assert!(app.mobile.sheet_open());
    play_frame(&mut app, &ctx, escape());
    play_frame(&mut app, &ctx, vec![]);
    assert!(!app.mobile.sheet_open());
    assert_eq!(app.mobile.nav.settings_page(), Some(SettingsTab::Sound));
}

#[test]
fn a_picked_value_applies_and_closes_the_sheet() {
    let (mut app, ctx) = settings_page("sheet-pick", SettingsTab::General);
    app.ui.settings.ui_theme = crate::app::ui_state::ui_theme::UiTheme::Light;
    let output = play_frame(&mut app, &ctx, vec![]);
    let theme = text_rect(&output, "Theme").expect("a theme row");
    play_frame(&mut app, &ctx, tap(theme.center()));
    play_frame(&mut app, &ctx, vec![]);
    let output = play_frame(&mut app, &ctx, vec![]);
    let dark = last_text_rect(&output, "Dark").expect("a dark choice");
    play_frame(&mut app, &ctx, tap(dark.center()));
    play_frame(&mut app, &ctx, vec![]);
    assert_eq!(
        app.ui.settings.ui_theme,
        crate::app::ui_state::ui_theme::UiTheme::Dark
    );
    assert!(!app.mobile.sheet_open());
}

#[test]
fn switch_labels_sit_mid_row_and_a_tap_on_them_flips_the_switch() {
    let (mut app, ctx) = settings_page("switch-row", SettingsTab::Controls);
    let output = play_frame(&mut app, &ctx, vec![]);
    let show = text_rect(&output, "Show").expect("a show row").center().y;
    let vibrate = text_rect(&output, "Vibrate on press")
        .expect("a vibrate row")
        .center()
        .y;
    let size = text_rect(&output, "Size").expect("a size row").center().y;
    assert!(
        ((size - vibrate) - (vibrate - show)).abs() < 3.0,
        "switch labels at {show} and {vibrate}, the next plain row at {size}"
    );
    let before = app.ui.settings.touch.show_controls;
    let label = text_rect(&output, "Show").unwrap();
    play_frame(&mut app, &ctx, tap(label.center()));
    play_frame(&mut app, &ctx, vec![]);
    assert_ne!(app.ui.settings.touch.show_controls, before);
}

fn signed_in_you(name: &str) -> (Citrine, Context) {
    let ctx = Context::default();
    let mut app = phone_app(name);
    let account = citrine_gb_net::Account {
        online_id: citrine_gb_net::OnlineId::new(0x1234_5678).unwrap(),
        display_name: "Clara".into(),
        storage_used: 0,
        quota_bytes: 0,
        two_factor: false,
        codes_left: 0,
    };
    app.ui.settings.online.remember(
        &crate::online::address::ServerAddress::parse("relay.example").unwrap(),
        &citrine_gb_net::RelayId::random(),
        "pw",
        &account,
        &Default::default(),
    );
    app.mobile.nav.go(Place::You);
    play_frame(&mut app, &ctx, vec![]);
    (app, ctx)
}

#[test]
fn the_sign_in_key_sheet_fits_on_the_screen() {
    let (mut app, ctx) = signed_in_you("you-key-fits");
    let output = play_frame(&mut app, &ctx, vec![]);
    let row = text_rect(&output, "Sign-in key").expect("a sign-in key row");
    play_frame(&mut app, &ctx, tap(row.center()));
    for _ in 0..3 {
        play_frame(&mut app, &ctx, vec![]);
    }
    let output = play_frame(&mut app, &ctx, vec![]);
    let copy = output
        .shapes
        .iter()
        .rev()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.text().ends_with("Copy sign-in key") => {
                Some(text.visual_bounding_rect())
            }
            _ => None,
        })
        .expect("a copy button");
    assert!(
        copy.bottom() <= ctx.content_rect().bottom(),
        "the copy button ends at {} under a screen bottom of {}",
        copy.bottom(),
        ctx.content_rect().bottom()
    );
}

#[test]
fn a_sheet_note_starts_under_its_title_and_the_button_label_is_centred() {
    let (mut app, ctx) = signed_in_you("you-key-lines-up");
    let output = play_frame(&mut app, &ctx, vec![]);
    let row = text_rect(&output, "Sign-in key").expect("a sign-in key row");
    play_frame(&mut app, &ctx, tap(row.center()));
    for _ in 0..3 {
        play_frame(&mut app, &ctx, vec![]);
    }
    let output = play_frame(&mut app, &ctx, vec![]);
    let title = last_text_rect(&output, "Sign-in key").expect("a sheet title");
    let find = |wanted: fn(&str) -> bool| {
        output
            .shapes
            .iter()
            .rev()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if wanted(text.galley.text()) => {
                    Some(text.visual_bounding_rect())
                }
                _ => None,
            })
    };
    let note = find(|text| text.starts_with("Use it to sign in")).expect("a sheet note");
    let copy = find(|text| text.ends_with("Copy sign-in key")).expect("a copy button");
    assert!(
        (note.left() - title.left()).abs() < 3.0,
        "the note starts at {} and the title at {}",
        note.left(),
        title.left()
    );
    assert!(
        2.0 * (note.top() - title.bottom()) < copy.top() - note.bottom(),
        "the note sits {} under its title and {} over the button",
        note.top() - title.bottom(),
        copy.top() - note.bottom()
    );
    let middle = ctx.content_rect().center().x;
    assert!(
        (copy.center().x - middle).abs() < 3.0,
        "the button label is centred on {} in a sheet centred on {}",
        copy.center().x,
        middle
    );
}

#[test]
fn signing_out_on_a_phone_asks_in_a_sheet() {
    let (mut app, ctx) = signed_in_you("you-sign-out-sheet");
    let output = play_frame(&mut app, &ctx, vec![]);
    let row = text_rect(&output, "Sign out").expect("a sign out row");
    play_frame(&mut app, &ctx, tap(row.center()));
    for _ in 0..3 {
        play_frame(&mut app, &ctx, vec![]);
    }
    let question = |output: &egui::FullOutput| {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text)
                    if text.galley.text().starts_with("Sign out on this device") =>
                {
                    Some(text.visual_bounding_rect())
                }
                _ => None,
            })
    };
    let output = play_frame(&mut app, &ctx, vec![]);
    let asked = question(&output).expect("a sign out question");
    assert!(
        asked.top() > ctx.content_rect().center().y,
        "the question starts at {}, above the screen middle, so it isn't a bottom sheet",
        asked.top()
    );
    let cancel = last_text_rect(&output, "Cancel").expect("a cancel button");
    play_frame(&mut app, &ctx, tap(cancel.center()));
    for _ in 0..2 {
        play_frame(&mut app, &ctx, vec![]);
    }
    let output = play_frame(&mut app, &ctx, vec![]);
    assert!(question(&output).is_none(), "cancel closes the question");
}

#[test]
fn tapping_the_online_id_copies_it() {
    let (mut app, ctx) = signed_in_you("you-copy");
    let output = play_frame(&mut app, &ctx, vec![]);
    let row = text_rect(&output, "Online ID").expect("an online id row");
    let output = play_frame(&mut app, &ctx, tap(row.center()));
    let copied =
        output.platform_output.commands.iter().any(
            |command| matches!(command, egui::OutputCommand::CopyText(text) if !text.is_empty()),
        );
    assert!(copied, "the online id goes to the clipboard");
    let output = play_frame(&mut app, &ctx, vec![]);
    assert!(text_rect(&output, "Copied").is_some());
}

#[test]
fn back_closes_the_sign_in_key_sheet_and_stays_on_you() {
    let (mut app, ctx) = signed_in_you("you-key");
    let output = play_frame(&mut app, &ctx, vec![]);
    let row = text_rect(&output, "Sign-in key").expect("a sign-in key row");
    play_frame(&mut app, &ctx, tap(row.center()));
    play_frame(&mut app, &ctx, vec![]);
    assert!(app.mobile.sheet_open());
    play_frame(&mut app, &ctx, escape());
    play_frame(&mut app, &ctx, vec![]);
    assert!(!app.mobile.sheet_open());
    assert_eq!(app.mobile.nav.place(), Place::You);
}

#[test]
fn the_docked_debugger_shows_breakpoints_above_a_visible_cpu_header() {
    use citrine_gb::debug::breakpoints::{Access, Watch};
    use citrine_gb::debug::code::{Address, BlockId};
    for cpu_open in [false, true] {
        let ctx = Context::default();
        let mut app = phone_app(&format!("debugger-stops-{cpu_open}"));
        app.ui.settings.layout = LayoutMode::Desktop;
        app.ui.settings.ui_scale = 2.2;
        app.ui.registers.docked_cpu = cpu_open;
        load_tobu(&mut app);
        let gb = app.emulator.as_gb_mut().unwrap();
        gb.debugger
            .breakpoints
            .insert(Address::at(BlockId::Rom, 0, 0x0150));
        gb.debugger.breakpoints.watch(Watch {
            from: Address::at(BlockId::Wram, 0, 0),
            len: 1,
            access: Access::Write,
        });
        app.open_tab(Tab::Disassembly);
        let output = settle(&mut app, &ctx, egui::vec2(1280.0, 800.0), 1.0);
        let shown = |wanted: &str| {
            output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if text.galley.text().trim() == wanted => {
                        let rect = text.visual_bounding_rect();
                        Some((rect, clipped.clip_rect))
                    }
                    _ => None,
                })
        };
        let (list, list_clip) = shown("Breakpoints").expect("a breakpoint list");
        let (cpu, cpu_clip) = shown("CPU").expect("a CPU header");
        assert!(
            list_clip.contains_rect(list),
            "CPU open {cpu_open}: the list at {list:?} is cut by {list_clip:?}"
        );
        assert!(
            cpu_clip.contains_rect(cpu),
            "CPU open {cpu_open}: the CPU header at {cpu:?} is cut by {cpu_clip:?}"
        );
        assert!(
            list.bottom() <= cpu.top(),
            "CPU open {cpu_open}: {list:?} {cpu:?}"
        );
        for entry in ["00:0150", "00:C000"] {
            let listed = output.shapes.iter().any(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text().trim() == entry => {
                    let rect = text.visual_bounding_rect();
                    let side = ctx.style().spacing.interact_size.y;
                    clipped.clip_rect.contains_rect(rect)
                        && clipped.clip_rect.height() >= side - 0.5
                        && rect.top() >= list.bottom()
                        && rect.bottom() <= cpu.top()
                }
                _ => false,
            });
            let found: Vec<(egui::Rect, egui::Rect)> = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if text.galley.text().trim() == entry => {
                        Some((text.visual_bounding_rect(), clipped.clip_rect))
                    }
                    _ => None,
                })
                .collect();
            assert!(
                listed,
                "CPU open {cpu_open}: {entry} is not in view: {found:?}, side {}, list {list:?} in {list_clip:?}, cpu {cpu:?} in {cpu_clip:?}, floor {:?}",
                ctx.style().spacing.interact_size.y,
                ctx.data(|data| data.get_temp::<(f32, f32)>(egui::Id::new("debugger_stops_height")))
            );
        }
    }
}

fn signed_out_you(name: &str, server: bool) -> (Citrine, Context) {
    let (mut app, ctx) = signed_in_you(name);
    app.ui.settings.online.leave();
    if !server {
        app.ui.settings.online.address.clear();
        app.ui.settings.online.relays.clear();
    }
    play_frame(&mut app, &ctx, vec![]);
    (app, ctx)
}

#[test]
fn signed_out_you_shows_no_account_and_no_avatar() {
    let (mut app, ctx) = signed_out_you("you-signed-out", true);
    let output = play_frame(&mut app, &ctx, vec![]);
    assert!(text_rect(&output, "Sign in").is_some());
    assert!(text_rect(&output, "Create account").is_some());
    for gone in ["Clara", "C", "Sign out", "Online ID"] {
        assert!(
            text_rect(&output, gone).is_none(),
            "{gone} shows signed out"
        );
    }
}

#[test]
fn signed_out_you_without_a_server_says_so_in_one_row() {
    let (mut app, ctx) = signed_out_you("you-no-server", false);
    let output = play_frame(&mut app, &ctx, vec![]);
    assert!(text_rect(&output, "Not set up").is_some());
    assert!(text_rect(&output, "Sign in").is_none());
    let row = text_rect(&output, "Server").expect("a server row");
    play_frame(&mut app, &ctx, tap(row.center()));
    play_frame(&mut app, &ctx, vec![]);
    let output = play_frame(&mut app, &ctx, vec![]);
    assert!(app.mobile.sheet_open());
    assert!(text_rect(&output, "Address").is_some());
}

#[test]
fn the_sign_in_sheet_closes_with_back_and_stays_on_you() {
    let (mut app, ctx) = signed_out_you("you-sign-in-sheet", true);
    for row_label in ["Sign in", "Create account", "Server"] {
        let output = play_frame(&mut app, &ctx, vec![]);
        let row = text_rect(&output, row_label).expect("a signed out row");
        play_frame(&mut app, &ctx, tap(row.center()));
        play_frame(&mut app, &ctx, vec![]);
        assert!(app.mobile.sheet_open(), "{row_label} opens a sheet");
        play_frame(&mut app, &ctx, escape());
        play_frame(&mut app, &ctx, vec![]);
        assert!(
            !app.mobile.sheet_open(),
            "back closes the {row_label} sheet"
        );
        assert_eq!(app.mobile.nav.place(), Place::You);
    }
}

fn with_second_account(app: &mut Citrine) {
    let account = citrine_gb_net::Account {
        online_id: citrine_gb_net::OnlineId::new(0x2345_6789).unwrap(),
        display_name: "Zitrone".into(),
        storage_used: 0,
        quota_bytes: 0,
        two_factor: false,
        codes_left: 0,
    };
    let relay = crate::online::address::ServerAddress::parse("relay.example").unwrap();
    app.ui.settings.online.leave();
    app.ui.settings.online.remember(
        &relay,
        &citrine_gb_net::RelayId::random(),
        "pw",
        &account,
        &Default::default(),
    );
}

#[test]
fn the_sign_in_sheet_offers_the_saved_accounts_and_signs_in_with_one_tap() {
    let (mut app, ctx) = signed_out_you("you-saved-accounts", true);
    let output = play_frame(&mut app, &ctx, vec![]);
    assert!(
        text_rect(&output, "Clara").is_none(),
        "no account on the signed out page"
    );
    let row = text_rect(&output, "Sign in").expect("a sign in row");
    play_frame(&mut app, &ctx, tap(row.center()));
    play_frame(&mut app, &ctx, vec![]);
    let output = play_frame(&mut app, &ctx, vec![]);
    let saved = text_rect(&output, "Clara").expect("the saved account in the sheet");
    play_frame(&mut app, &ctx, tap(saved.center()));
    play_frame(&mut app, &ctx, vec![]);
    assert!(app.ui.settings.online.is_joined());
    assert_eq!(app.ui.settings.online.address, "relay.example");
}

#[test]
fn switch_account_lists_the_accounts_switches_and_adds_one() {
    let (mut app, ctx) = signed_in_you("you-switch");
    with_second_account(&mut app);
    play_frame(&mut app, &ctx, vec![]);
    let output = play_frame(&mut app, &ctx, vec![]);
    let row = text_rect(&output, "Switch account").expect("a switch account row");
    play_frame(&mut app, &ctx, tap(row.center()));
    play_frame(&mut app, &ctx, vec![]);
    let output = play_frame(&mut app, &ctx, vec![]);
    assert!(app.mobile.sheet_open());
    assert!(text_rect(&output, "Add account").is_some());
    play_frame(&mut app, &ctx, escape());
    play_frame(&mut app, &ctx, vec![]);
    assert!(!app.mobile.sheet_open(), "back closes the accounts sheet");

    let output = play_frame(&mut app, &ctx, vec![]);
    let row = text_rect(&output, "Switch account").unwrap();
    play_frame(&mut app, &ctx, tap(row.center()));
    play_frame(&mut app, &ctx, vec![]);
    let output = play_frame(&mut app, &ctx, vec![]);
    let other = text_rect(&output, "Clara").expect("the other account");
    play_frame(&mut app, &ctx, tap(other.center()));
    play_frame(&mut app, &ctx, vec![]);
    let joined = app.ui.settings.online.joined_entry().unwrap();
    assert_eq!(joined.display_name.as_deref(), Some("Clara"));
    assert_eq!(joined.others.len(), 1);
    assert!(!app.mobile.sheet_open());

    let output = play_frame(&mut app, &ctx, vec![]);
    let row = text_rect(&output, "Switch account").unwrap();
    play_frame(&mut app, &ctx, tap(row.center()));
    play_frame(&mut app, &ctx, vec![]);
    let output = play_frame(&mut app, &ctx, vec![]);
    let add = text_rect(&output, "Add account").unwrap();
    play_frame(&mut app, &ctx, tap(add.center()));
    play_frame(&mut app, &ctx, vec![]);
    let output = play_frame(&mut app, &ctx, vec![]);
    assert!(
        app.ui.settings.online.is_joined(),
        "add account keeps the session"
    );
    assert!(app.mobile.sheet_open());
    assert!(text_rect(&output, "Create account").is_some());
    let sign_in = text_rect(&output, "Sign in").expect("a sign in row");
    play_frame(&mut app, &ctx, tap(sign_in.center()));
    play_frame(&mut app, &ctx, vec![]);
    play_frame(&mut app, &ctx, vec![]);
    assert!(!app.ui.settings.online.is_joined());
    assert!(app.mobile.sheet_open(), "the sign in sheet opens");
    play_frame(&mut app, &ctx, escape());
    play_frame(&mut app, &ctx, vec![]);
    play_frame(&mut app, &ctx, vec![]);
    assert!(!app.mobile.sheet_open());
    let joined = app
        .ui
        .settings
        .online
        .joined_entry()
        .expect("back on the account");
    assert_eq!(joined.display_name.as_deref(), Some("Clara"));
    assert_eq!(app.ui.settings.online.accounts_on("relay.example").len(), 2);
}

#[test]
fn the_online_page_signed_out_shows_the_server_row() {
    let (mut app, ctx) = settings_page("online-signed-out", SettingsTab::Online);
    let output = play_frame(&mut app, &ctx, vec![]);
    assert!(text_rect(&output, "Not set up").is_some());
    assert!(text_rect(&output, "Relay password").is_none());
}

#[test]
fn the_frame_shows_friends_only_while_signed_in() {
    let (mut app, ctx) = signed_in_you("frame-friends");
    let output = play_frame(&mut app, &ctx, vec![]);
    assert!(text_rect(&output, "Friends").is_some());
    app.ui.settings.online.leave();
    play_frame(&mut app, &ctx, vec![]);
    let output = play_frame(&mut app, &ctx, vec![]);
    assert!(text_rect(&output, "Friends").is_none());
    app.mobile.nav.go(Place::Friends);
    play_frame(&mut app, &ctx, vec![]);
    play_frame(&mut app, &ctx, vec![]);
    assert_eq!(app.mobile.nav.place(), Place::You);
}

#[test]
fn an_offer_names_its_download_size() {
    let offer = crate::update::Offer {
        version: "0.11.0".to_owned(),
        page: String::new(),
        installable: true,
        size: 48_200_000,
    };
    let texts =
        super::widgets::update::texts(&crate::update::Phase::Available(offer), false).unwrap();
    assert_eq!(texts.title, "Citrine 0.11.0 is out");
    assert_eq!(texts.detail, "48.2 MB download");
    assert_eq!(texts.action, Some("Update"));
}

#[test]
fn the_video_tool_shows_every_view_of_a_game_boy_inside_its_tab() {
    use crate::app::widgets::video::VideoTab;
    let ctx = Context::default();
    let mut app = phone_app("video-views");
    app.ui.settings.layout = LayoutMode::Desktop;
    app.ui.settings.ui_scale = 2.2;
    load_tobu(&mut app);
    app.open_tab(Tab::Tool(tabs::Tool::Video));
    app.ui.video.show_hidden = true;
    for tab in VideoTab::ALL {
        app.ui.video.tab = tab;
        let output = settle(&mut app, &ctx, egui::vec2(1280.0, 800.0), 1.0);
        let images: Vec<_> = output
            .shapes
            .iter()
            .filter(|clipped| match &clipped.shape {
                egui::Shape::Mesh(mesh) => mesh.texture_id != egui::TextureId::default(),
                _ => false,
            })
            .collect();
        let shows_pictures = tab != VideoTab::Palettes;
        assert_eq!(!images.is_empty(), shows_pictures, "{tab:?}");
        for clipped in images {
            let egui::Shape::Mesh(mesh) = &clipped.shape else {
                continue;
            };
            let rect = mesh.calc_bounds();
            assert!(
                rect.left() >= clipped.clip_rect.left() - 0.5
                    && rect.right() <= clipped.clip_rect.right() + 0.5,
                "{tab:?}: picture {rect:?} is wider than its tab {:?}",
                clipped.clip_rect
            );
        }
    }
    assert_eq!(app.ui.video.sprites().len(), 40);
    assert_eq!(app.ui.video.maps().len(), 2);
    assert_eq!(app.ui.video.palettes().len(), 2);
    app.ui.video.tab = VideoTab::Sprites;
    let source = app.ui.video.sprites()[0].source;
    app.ui.video.select(source);
    assert_eq!(app.ui.video.tab, VideoTab::Tiles);
    assert_eq!(app.ui.video.selected, Some(source));
    assert_eq!(app.ui.video.back, Some(VideoTab::Sprites));
}

#[cfg(feature = "gba")]
#[test]
fn the_video_tool_shows_a_gba_with_its_own_sheets_and_sprites() {
    use crate::app::widgets::video::VideoTab;
    let ctx = Context::default();
    let mut app = phone_app("video-gba");
    app.ui.settings.layout = LayoutMode::Desktop;
    app.emulator.load_gba(&[0; 0x200]).expect("a gba loads");
    app.open_tab(Tab::Tool(tabs::Tool::Video));
    for tab in VideoTab::ALL {
        app.ui.video.tab = tab;
        settle(&mut app, &ctx, egui::vec2(1280.0, 800.0), 1.0);
    }
    assert_eq!(app.ui.video.sprites().len(), 128);
    assert_eq!(app.ui.video.palettes()[0].colors.len(), 256);
    app.ui.video.select(app.ui.video.sprites()[0].source);
    settle(&mut app, &ctx, egui::vec2(1280.0, 800.0), 1.0);
    assert_eq!(app.ui.video.sheet, 2);
    assert_eq!(
        app.ui.video.colors,
        citrine_gb::debug::video::SheetColors::Palette { group: 1, row: 0 }
    );
}

#[cfg(feature = "gba")]
mod gba_tools;

#[test]
fn the_debugger_lays_out_side_by_side_in_the_right_pane_beside_another_tab() {
    for (scale, pane) in [(1.0, 505.0), (1.113, 452.9)] {
        let ctx = Context::default();
        let mut app = phone_app(&format!("debugger-right-pane-{scale}"));
        app.ui.settings.layout = LayoutMode::Desktop;
        app.ui.settings.ui_scale = scale;
        load_tobu(&mut app);
        app.open_tab(Tab::Disassembly);
        settle(&mut app, &ctx, egui::vec2(1280.0, 800.0), 1.0);
        let output = settle(&mut app, &ctx, egui::vec2(1280.0, 800.0), 1.0);

        let (_, node, _) = app.dock.find_tab(&Tab::Disassembly).expect("docked");
        let egui_dock::Node::Leaf(leaf) = &app.dock.main_surface()[node] else {
            panic!("the debugger sits in a leaf");
        };
        assert!(leaf.tabs().len() > 1, "the debugger shares its pane");
        assert!((leaf.viewport.width() - pane).abs() < 1.0);
        let right = leaf.viewport.right();
        let af = output.shapes.iter().any(|clipped| match &clipped.shape {
            egui::Shape::Text(text) => {
                text.galley.text() == "AF"
                    && text.pos.x > leaf.viewport.left()
                    && clipped.clip_rect.contains_rect(text.visual_bounding_rect())
            }
            _ => false,
        });
        assert!(af, "the CPU column draws beside the code at {pane}");
        for clipped in &output.shapes {
            if let egui::Shape::Text(text) = &clipped.shape {
                assert!(
                    text.pos.x <= right,
                    "{:?} starts at {} past the pane's edge {right} at {pane}",
                    text.galley.text(),
                    text.pos.x
                );
            }
        }
    }
}

fn picture_rect(output: &egui::FullOutput) -> Option<egui::Rect> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default() => {
                Some(clipped.shape.visual_bounding_rect())
            }
            egui::Shape::Rect(rect) if rect.fill_texture_id() != egui::TextureId::default() => {
                Some(rect.rect)
            }
            _ => None,
        })
        .max_by(|a, b| a.area().total_cmp(&b.area()))
}

fn toast_frames(app: &mut Citrine, ctx: &Context, size: egui::Vec2) -> egui::FullOutput {
    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
        ..Default::default()
    };
    let mut output = None;
    for _ in 0..4 {
        output = Some(ctx.run(input(), |ctx| {
            mobile::touch_spacing(ctx, false);
            app.render(ctx);
            app.show_toasts(ctx);
        }));
    }
    output.unwrap()
}

#[test]
fn on_desktop_no_toast_covers_the_game_picture_at_any_window_size() {
    let sizes = [
        (1024.0, 768.0),
        (1280.0, 800.0),
        (1920.0, 1080.0),
        (800.0, 600.0),
        (640.0, 480.0),
        (1280.0 / 2.2, 720.0 / 2.2),
        (1024.0 / 2.2, 640.0 / 2.2),
        (1920.0 / 2.2, 1000.0 / 2.2),
    ];
    let progress = crate::progress::Progress::bytes(700 * 1024, 2500 * 1024);
    let start = web_time::Instant::now();
    let later = start + std::time::Duration::from_secs(1);
    for (width, height) in sizes {
        for two_lines in [false, true] {
            for homebrew in [false, true] {
                let name = format!("toast-{width}-{two_lines}-{homebrew}");
                let mut app = crystal_app(&name, LayoutMode::Desktop, None);
                app.emulator.running = true;
                if homebrew {
                    app.open_tab(Tab::Homebrew);
                }
                let ctx = Context::default();
                app.emulator.clear_frame(&ctx);
                let bare = toast_frames(&mut app, &ctx, egui::vec2(width, height));
                let bare = picture_rect(&bare).expect("the game is drawn");
                if two_lines {
                    for now in [start, later] {
                        app.toasts
                            .load("shelf", "Downloading Hyperspace Roll", progress, now);
                    }
                }
                app.toasts.error("Couldn't download Pentris").button(
                    "Try again",
                    events::AppEvent::OpenTab { tab: Tab::Homebrew },
                );
                let output = toast_frames(&mut app, &ctx, egui::vec2(width, height));
                let picture = picture_rect(&output).expect("the game is drawn");
                assert!(
                    picture.height() >= bare.height() * 0.75,
                    "{name}: {picture:?} shrank from {bare:?}"
                );
                let toasts = app.toasts.rects(&ctx);
                assert_eq!(toasts.len(), 1 + two_lines as usize, "{name}");
                for toast in toasts {
                    assert!(
                        !toast.shrink(0.5).intersects(picture),
                        "{name}: {toast:?} covers {picture:?}"
                    );
                    assert!(
                        ctx.content_rect().contains_rect(toast),
                        "{name}: {toast:?} leaves the window"
                    );
                }
            }
        }
    }
}

#[test]
fn on_desktop_the_invite_banner_keeps_most_of_the_game_picture() {
    for zoom in [1.0, 2.2] {
        let name = format!("invite-picture-{zoom}");
        let mut app = crystal_app(&name, LayoutMode::Desktop, None);
        app.emulator.running = true;
        let ctx = Context::default();
        app.emulator.clear_frame(&ctx);
        let heights: Vec<f32> = (0..32)
            .map(|step| (560.0 + step as f32 * 16.0) / zoom)
            .collect();
        let size = |height: f32| egui::vec2(1280.0 / zoom, height);
        let bare: Vec<egui::Rect> = heights
            .iter()
            .map(|&height| {
                let output = toast_frames(&mut app, &ctx, size(height));
                picture_rect(&output).expect("the game is drawn")
            })
            .collect();
        app.play.push_invite(crate::play::IncomingInvite {
            session: 1,
            from: citrine_gb_net::OnlineId::new(7).expect("id"),
            game: None,
        });
        for (&height, bare) in heights.iter().zip(bare) {
            let output = toast_frames(&mut app, &ctx, size(height));
            let picture = picture_rect(&output).expect("the game is drawn");
            assert!(
                picture.height() >= bare.height() * 0.75,
                "{name} at {height}: {picture:?} shrank from {bare:?}"
            );
        }
    }
}

fn shows_text(app: &mut Citrine, wanted: &str) -> bool {
    let ctx = Context::default();
    settle(app, &ctx, egui::vec2(412.0, 915.0), 2.0)
        .shapes
        .into_iter()
        .any(|clipped| matches!(&clipped.shape, egui::Shape::Text(text) if text.galley.text() == wanted))
}

#[test]
fn more_games_gets_a_heading_only_below_other_games() {
    let mut first = phone_app("more-games-first");
    assert!(
        !shows_text(&mut first, "More games"),
        "a first visit has nothing for More to follow"
    );
    let mut playing = phone_app("more-games-playing");
    load_game(
        &mut playing,
        &citrine_gb_gamedata::fixtures::crystal_cartridge(),
        None,
    );
    playing.mobile.nav.go(Place::Games);
    assert!(shows_text(&mut playing, "Now playing"));
    assert!(shows_text(&mut playing, "More games"));
}

fn hidden_game(name: &str) -> (Citrine, Context) {
    let ctx = Context::default();
    let mut app = phone_app(name);
    let mut rom = vec![0u8; 0x8000];
    rom[0x0147] = 0x03;
    rom[0x0149] = 0x02;
    app.emulator
        .load_rom(&citrine_gb::rom::Rom::new(&rom), GbModel::Dmg, None)
        .unwrap();
    app.mobile.nav.show_game();
    let mut output = play_frame(&mut app, &ctx, vec![]);
    for _ in 0..2 {
        output = play_frame(&mut app, &ctx, vec![]);
    }
    let hide =
        text_containing(&output, crate::icons::EYE_SLASH).expect("the strip has the hide button");
    play_frame(&mut app, &ctx, tap(hide.center()));
    (app, ctx)
}

fn frames(app: &mut Citrine, ctx: &Context, events: Vec<egui::Event>) -> egui::FullOutput {
    play_frame(app, ctx, events);
    play_frame(app, ctx, vec![]);
    play_frame(app, ctx, vec![])
}

fn escape_frames(app: &mut Citrine, ctx: &Context) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(412.0, 915.0),
        )),
        events: escape(),
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| {
        app.render(ctx);
        app.mobile_escape(ctx);
    });
    frames(app, ctx, vec![])
}

#[test]
fn the_eye_slash_hides_the_bar_and_the_strip_and_back_brings_them_back() {
    let (mut app, ctx) = hidden_game("hide-back");
    let output = frames(&mut app, &ctx, vec![]);
    assert!(app.mobile.ui_hidden());
    assert!(text_rect(&output, "Games").is_none());
    assert!(text_containing(&output, crate::icons::EYE_SLASH).is_none());
    assert!(text_containing(&output, crate::icons::EYE).is_some());
    let output = escape_frames(&mut app, &ctx);
    assert!(!app.mobile.ui_hidden());
    assert!(app.mobile.nav.on_game());
    assert!(text_rect(&output, "Games").is_some());
}

#[test]
fn the_eye_button_brings_the_ui_back() {
    let (mut app, ctx) = hidden_game("hide-eye");
    let output = frames(&mut app, &ctx, vec![]);
    let eye = text_containing(&output, crate::icons::EYE).expect("the eye button shows");
    let output = frames(&mut app, &ctx, tap(eye.center()));
    assert!(!app.mobile.ui_hidden());
    assert!(text_rect(&output, "Games").is_some());
}

#[test]
fn a_controller_press_hides_the_touch_buttons_until_a_touch_below_the_picture() {
    let (mut app, ctx) = hidden_game("hide-pad");
    let output = frames(&mut app, &ctx, vec![]);
    let select = text_rect(&output, "SELECT").expect("touch buttons show at first");
    app.emulator.press(
        crate::held::InputSource::PadButtons(0),
        citrine_gb::console::Keys::A,
    );
    app.emulator.release(
        crate::held::InputSource::PadButtons(0),
        citrine_gb::console::Keys::A,
    );
    let output = frames(&mut app, &ctx, vec![]);
    assert!(text_rect(&output, "SELECT").is_none());
    assert!(app.mobile.ui_hidden());
    let output = frames(&mut app, &ctx, tap(egui::pos2(30.0, select.center().y)));
    assert!(text_rect(&output, "SELECT").is_some());
}

#[test]
fn a_key_press_hides_the_touch_buttons_too() {
    let (mut app, ctx) = hidden_game("hide-key");
    let output = frames(&mut app, &ctx, vec![]);
    assert!(text_rect(&output, "SELECT").is_some());
    let key = |pressed| egui::Event::Key {
        key: egui::Key::ArrowLeft,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: Default::default(),
    };
    for event in [key(true), key(false)] {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(412.0, 915.0),
            )),
            events: vec![event],
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            app.render(ctx);
            let Citrine {
                emulator, pads, ui, ..
            } = &mut app;
            emulator.handle_input(
                ctx,
                pads.as_mut(),
                &ui.settings.bindings,
                &mut ui.capture,
                None,
            );
        });
    }
    let output = frames(&mut app, &ctx, vec![]);
    assert!(text_rect(&output, "SELECT").is_none());
}

#[test]
fn the_select_and_start_labels_fit_their_pills_at_the_biggest_size_in_landscape() {
    let ctx = Context::default();
    let mut app = phone_app("pill-labels");
    app.ui.settings.touch.scale = crate::app::ui_state::touch::TouchSettings::MAX_SCALE;
    let mut rom = vec![0u8; 0x8000];
    rom[0x0147] = 0x03;
    rom[0x0149] = 0x02;
    app.emulator
        .load_rom(&citrine_gb::rom::Rom::new(&rom), GbModel::Dmg, None)
        .unwrap();
    app.mobile.nav.show_game();
    let output = settle(&mut app, &ctx, egui::vec2(640.0, 360.0), 1.0);
    for label in ["SELECT", "START"] {
        let text = text_rect(&output, label).expect("the pill label shows");
        let pill = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(rect) if rect.rect.contains(text.center()) => Some(rect.rect),
                _ => None,
            })
            .min_by(|a, b| a.area().total_cmp(&b.area()))
            .expect("the pill shows behind its label");
        assert!(
            pill.contains_rect(text),
            "{label}: label {text:?} runs past its pill {pill:?}"
        );
    }
}

fn sized_frame(
    app: &mut Citrine,
    ctx: &Context,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
        events,
        ..Default::default()
    };
    ctx.run(input, |ctx| {
        let touch = app.is_mobile(ctx);
        mobile::touch_spacing(ctx, touch);
        app.render(ctx);
    })
}

fn game_at(name: &str, size: egui::Vec2, scale: f32, hide: bool) -> (Citrine, Context) {
    let ctx = Context::default();
    let mut app = phone_app(name);
    app.ui.settings.touch.scale = scale;
    app.emulator.scaling = app.ui.settings.touch.scaling;
    app.emulator.enable_matrix = app.ui.settings.matrix;
    let mut rom = vec![0u8; 0x8000];
    rom[0x0147] = 0x03;
    rom[0x0149] = 0x02;
    app.emulator
        .load_rom(&citrine_gb::rom::Rom::new(&rom), GbModel::Dmg, None)
        .unwrap();
    app.mobile.nav.show_game();
    let mut output = sized_frame(&mut app, &ctx, size, vec![]);
    for _ in 0..2 {
        output = sized_frame(&mut app, &ctx, size, vec![]);
    }
    if hide {
        let slash = text_containing(&output, crate::icons::EYE_SLASH)
            .expect("the strip has the hide button");
        sized_frame(&mut app, &ctx, size, tap(slash.center()));
        assert!(app.mobile.ui_hidden());
    }
    (app, ctx)
}

fn textured(shape: &egui::Shape) -> Option<egui::Rect> {
    match shape {
        egui::Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default() => {
            Some(mesh.calc_bounds())
        }
        egui::Shape::Rect(rect) if rect.fill_texture_id() != egui::TextureId::default() => {
            Some(rect.rect)
        }
        _ => None,
    }
}

fn covering(output: &egui::FullOutput, picture: egui::Rect) -> Vec<egui::Rect> {
    let inside = picture.shrink(0.5);
    output
        .shapes
        .iter()
        .filter(|clipped| textured(&clipped.shape).is_none())
        .map(|clipped| clipped.shape.visual_bounding_rect())
        .filter(|rect| rect.is_positive() && !rect.contains_rect(picture))
        .filter(|rect| rect.intersects(inside))
        .collect()
}

fn hidden_picture(name: &str, size: egui::Vec2, scale: f32) -> (egui::Rect, Vec<egui::Rect>) {
    let (mut app, ctx) = game_at(name, size, scale, true);
    let mut output = sized_frame(&mut app, &ctx, size, vec![]);
    for _ in 0..3 {
        output = sized_frame(&mut app, &ctx, size, vec![]);
    }
    assert!(
        text_containing(&output, crate::icons::EYE).is_some(),
        "the eye shows"
    );
    let picture = picture_rect(&output).expect("the game picture shows");
    (picture, covering(&output, picture))
}

#[test]
fn hidden_in_landscape_the_picture_takes_the_full_height_centred_with_nothing_over_it() {
    let size = egui::vec2(640.0, 360.0);
    for scale in [1.0, 1.6] {
        let (picture, over) = hidden_picture("fill-landscape", size, scale);
        assert!(
            (picture.height() - size.y).abs() <= 1.0,
            "Size {scale}: picture {picture:?} is not full height"
        );
        assert!((picture.center().y - size.y / 2.0).abs() <= 1.0);
        assert!((picture.center().x - size.x / 2.0).abs() <= 1.0);
        assert!(over.is_empty(), "Size {scale}: {over:?} cover {picture:?}");
    }
}

#[test]
fn hidden_in_portrait_the_picture_spans_the_full_width_at_the_top_with_nothing_over_it() {
    let size = egui::vec2(360.0, 640.0);
    for scale in [1.0, 1.6] {
        let (picture, over) = hidden_picture("fill-portrait", size, scale);
        assert!(
            (picture.width() - size.x).abs() <= 1.0,
            "Size {scale}: picture {picture:?} is not full width"
        );
        assert!(picture.top().abs() <= 1.0);
        assert!((picture.center().x - size.x / 2.0).abs() <= 1.0);
        assert!(over.is_empty(), "Size {scale}: {over:?} cover {picture:?}");
    }
}

#[test]
fn a_picture_that_cannot_take_the_full_height_sits_centred_shown_or_hidden() {
    let size = egui::vec2(480.0, 400.0);
    for hide in [false, true] {
        for scale in [1.0, 1.6] {
            let (mut app, ctx) = game_at("fill-centred", size, scale, hide);
            let mut output = sized_frame(&mut app, &ctx, size, vec![]);
            for _ in 0..3 {
                output = sized_frame(&mut app, &ctx, size, vec![]);
            }
            let picture = picture_rect(&output).expect("the game picture shows");
            assert!(picture.height() < size.y - 1.0);
            let top = if hide { 0.0 } else { 48.0 };
            let middle = (top + size.y) / 2.0;
            assert!(
                (picture.center().y - middle).abs() <= 1.0,
                "hidden {hide}, Size {scale}: picture {picture:?} not centred on {middle}"
            );
            let over = covering(&output, picture);
            assert!(
                over.is_empty(),
                "hidden {hide}, Size {scale}: {over:?} cover {picture:?}"
            );
        }
    }
}

#[test]
fn the_landscape_picture_keeps_its_place_when_a_key_press_hides_the_touch_buttons() {
    let size = egui::vec2(640.0, 360.0);
    let (mut app, ctx) = game_at("fill-keep", size, 1.0, true);
    sized_frame(&mut app, &ctx, size, vec![]);
    let output = sized_frame(&mut app, &ctx, size, vec![]);
    let before = picture_rect(&output).expect("the game picture shows");
    assert!(text_rect(&output, "SELECT").is_some());
    app.emulator.press(
        crate::held::InputSource::PadButtons(0),
        citrine_gb::console::Keys::A,
    );
    app.emulator.release(
        crate::held::InputSource::PadButtons(0),
        citrine_gb::console::Keys::A,
    );
    let mut output = sized_frame(&mut app, &ctx, size, vec![]);
    for _ in 0..2 {
        output = sized_frame(&mut app, &ctx, size, vec![]);
    }
    assert!(text_rect(&output, "SELECT").is_none());
    assert_eq!(
        picture_rect(&output).expect("the game picture shows"),
        before
    );
}

fn wheel_over(
    app: &mut Citrine,
    ctx: &Context,
    size: egui::Vec2,
    at: egui::Pos2,
) -> egui::FullOutput {
    let wheel = egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -40.0),
        modifiers: egui::Modifiers::NONE,
    };
    sized_frame(app, ctx, size, vec![egui::Event::PointerMoved(at)]);
    let mut output = sized_frame(app, ctx, size, vec![wheel]);
    for _ in 0..40 {
        output = sized_frame(app, ctx, size, Vec::new());
    }
    output
}

fn settings_scroll_app(name: &str) -> (Citrine, Context) {
    let mut app = media_app(name, LayoutMode::Desktop);
    app.dock = DockState::new(vec![Tab::Settings]);
    app.ui.settings.current_tab = SettingsTab::General;
    (app, Context::default())
}

fn bar_and_page(
    app: &mut Citrine,
    ctx: &Context,
    size: egui::Vec2,
    over: Option<egui::Pos2>,
) -> (Option<egui::Rect>, Option<egui::Rect>) {
    let general = format!(
        "{} {}",
        SettingsTab::General.icon(),
        SettingsTab::General.title()
    );
    let output = match over {
        Some(at) => wheel_over(app, ctx, size, at),
        None => {
            let mut output = sized_frame(app, ctx, size, Vec::new());
            for _ in 0..3 {
                output = sized_frame(app, ctx, size, Vec::new());
            }
            output
        }
    };
    (text_rect(&output, &general), text_rect(&output, "Console"))
}

#[test]
fn the_settings_bar_and_page_scroll_on_their_own() {
    let (mut app, ctx) = settings_scroll_app("settings-scroll-page");
    let size = egui::vec2(1280.0, 400.0);
    let (tab, label) = bar_and_page(&mut app, &ctx, size, None);
    let (tab, label) = (tab.expect("the bar's first tab"), label.expect("the label"));
    let (tab_after, label_after) = bar_and_page(&mut app, &ctx, size, Some(label.center()));
    assert_eq!(tab_after.map(|r| r.top()), Some(tab.top()));
    let moved = label_after.map_or(f32::INFINITY, |r| label.top() - r.top());
    assert!(moved > 20.0, "page moved {moved}");

    let (mut app, ctx) = settings_scroll_app("settings-scroll-bar");
    let size = egui::vec2(1280.0, 140.0);
    let (tab, label) = bar_and_page(&mut app, &ctx, size, None);
    let (tab, label) = (tab.expect("the bar's first tab"), label.expect("the label"));
    let (tab_after, label_after) = bar_and_page(&mut app, &ctx, size, Some(tab.center()));
    assert_eq!(label_after.map(|r| r.top()), Some(label.top()));
    let moved = tab_after.map_or(f32::INFINITY, |r| tab.top() - r.top());
    assert!(moved > 20.0, "bar moved {moved}");
}

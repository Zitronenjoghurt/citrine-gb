use crate::app::tabs::TabViewer;
use crate::app::ui_state::per_game::Scope;
use crate::app::ui_state::settings::Settings;
use crate::app::ui_state::settings_tree::{SettingsTab, Surface};
use crate::app::widgets::settings::content::{Device, SettingsContent};
use crate::app::widgets::settings::page_list::{self, NARROW_BELOW, PageList, Pick};
use crate::app::widgets::settings::scope;
use crate::icons;
use egui::{RichText, Widget};

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    crate::app::widgets::update::card(ui, &mut viewer.ui.update, &viewer.files.spawner);
    let settings = &mut viewer.ui.settings;
    if !settings.current_tab.available(settings.dev_mode) {
        settings.current_tab = SettingsTab::default();
    }
    if settings.find.is_some() || settings.find_group.is_some() {
        viewer.ui.settings_list_open = false;
    }
    if ui.available_width() < NARROW_BELOW {
        if viewer.ui.settings.focus_search {
            viewer.ui.settings_list_open = true;
        }
        narrow(viewer, ui);
    } else {
        viewer.ui.settings_list_open = false;
        side_by_side(viewer, ui);
    }
}

fn side_by_side(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    let height = ui.available_height();
    let dev_mode = viewer.ui.settings.dev_mode;
    let list_width = page_list::width(ui, dev_mode);

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(list_width);
            ui.set_height(height);
            let settings = &mut viewer.ui.settings;
            let selected = Some(settings.current_tab);
            if let Some(pick) = page_list(settings, selected).hits_elsewhere().show(ui) {
                if matches!(pick, Pick::Row(_)) {
                    settings.search.clear();
                }
                follow(settings, pick);
            }
        });

        ui.separator();

        let settings = &mut viewer.ui.settings;
        if settings.search.trim().is_empty() {
            ui.vertical_centered(|ui| content(viewer, ui, 0.0));
        } else if let Some(row) = page_list::hits_pane(ui, &settings.search, dev_mode) {
            settings.search.clear();
            follow(settings, Pick::Row(row));
        }
    });
}

fn narrow(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    let settings = &mut viewer.ui.settings;
    if viewer.ui.settings_list_open {
        if let Some(pick) = page_list(settings, None).show(ui) {
            follow(settings, pick);
            viewer.ui.settings_list_open = false;
        }
        return;
    }

    let title = settings.current_tab.title();
    let back = ui
        .horizontal(|ui| {
            let back = ui
                .button(icons::CARET_LEFT)
                .on_hover_text("All settings")
                .clicked();
            ui.label(RichText::new(title).strong());
            back
        })
        .inner;
    if back {
        viewer.ui.settings_list_open = true;
        return;
    }
    let line = ui.separator().rect.center().y;
    let docked = !viewer.ui.settings.layout.is_touch(ui.ctx());
    let stroke = ui.visuals().widgets.noninteractive.bg_stroke.width;
    let mut below = ui.clip_rect().bottom() - ui.max_rect().bottom();
    if docked {
        below -= (stroke / 2.0).ceil() * 2.0;
    }
    let below = below.max(0.0);
    let page = egui::Rect::from_min_max(
        egui::pos2(ui.max_rect().left(), line + stroke / 2.0),
        ui.max_rect().max,
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(page), |ui| {
        content(viewer, ui, below)
    });
}

fn page_list(settings: &mut Settings, selected: Option<SettingsTab>) -> PageList<'_> {
    let focus = std::mem::take(&mut settings.focus_search);
    PageList::new(selected, settings.dev_mode, &mut settings.search).focus(focus)
}

fn follow(settings: &mut Settings, pick: Pick) {
    match pick {
        Pick::Page(page) => settings.current_tab = page,
        Pick::Row(row) => {
            if !row.per_game() {
                settings.leave_scope();
            }
            settings.current_tab = row.page();
            settings.find = Some(row);
        }
    }
}

pub(crate) fn content(viewer: &mut TabViewer, ui: &mut egui::Ui, top_space: f32) {
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    let discord = (
        crate::discord::game_now(viewer.emulator, viewer.stats, viewer.game_info),
        viewer.emulator.screen_texture(),
        viewer.online.pictures_off(),
    );
    let device = Device {
        light_sensor: viewer.senses.has_light_sensor(),
        live_camera: viewer.senses.has_live_camera(),
        camera_error: viewer.senses.camera_error(),
    };
    #[cfg(feature = "nds")]
    let ds_account = viewer
        .online
        .account()
        .map(|account| account.display_name.clone());
    let s = &viewer.ui.settings;
    let leave = s.scope() == Scope::ThisGame
        && scope::shows_bar(s, s.current_tab, Surface::Desktop)
        && super::chat::take_escape(ui.ctx());
    let tab = viewer.ui.settings.current_tab;
    let frame = scope::begin(&mut viewer.ui.settings, tab, Surface::Desktop);
    let content = SettingsContent::new(
        &mut viewer.ui.settings,
        &mut viewer.ui.capture,
        &mut viewer.ui.palette_editor,
        viewer.audio.as_ref(),
        Some(viewer.events),
    )
    .online(viewer.online, &mut viewer.ui.join)
    .your_data(&mut viewer.ui.your_data)
    .storage(&mut viewer.ui.storage)
    .info(&mut viewer.ui.info)
    .device(device)
    .top_space(top_space)
    .boot_roms(
        [
            crate::app::boot_rom::BootRom::Gb,
            crate::app::boot_rom::BootRom::Gbc,
        ]
        .map(|file| viewer.emulator.boot_rom_name(file)),
    );
    #[cfg(feature = "gba")]
    let content = content.gba_bios(viewer.emulator.gba_bios_name());
    #[cfg(feature = "nds")]
    let content = {
        use crate::app::ds_firmware::DsFile;
        content.ds(crate::app::widgets::settings::ds::DsState {
            account: ds_account.as_deref(),
            files: [DsFile::Arm9Bios, DsFile::Arm7Bios, DsFile::Firmware]
                .map(|file| viewer.emulator.ds_file_name(file)),
            mic: viewer.mic_test,
        })
    };
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    let content = content.discord(discord.0, discord.1, discord.2);
    content.ui(ui);
    viewer.ui.settings.end_scope(frame);
    if leave {
        viewer.ui.settings.leave_scope();
    }
}

#[cfg(test)]
mod tests {
    use crate::app::Citrine;
    use crate::app::tabs::Tab;
    use crate::app::tests::{phone_app, text_rect};
    use crate::app::ui_state::settings::LayoutMode;
    use crate::app::ui_state::settings_tree::Surface;
    use crate::app::ui_state::settings_tree::rows::RowId;
    use egui::{Context, Event, PointerButton, Pos2, RawInput, Rect, vec2};
    use egui_dock::{DockState, NodeIndex};

    const HEIGHT: f32 = 360.0;

    fn frame(app: &mut Citrine, ctx: &Context, events: Vec<Event>) -> egui::FullOutput {
        tall_frame(app, ctx, events, HEIGHT)
    }

    fn tall_frame(
        app: &mut Citrine,
        ctx: &Context,
        events: Vec<Event>,
        height: f32,
    ) -> egui::FullOutput {
        sized_frame(app, ctx, events, 1280.0, height)
    }

    fn sized_frame(
        app: &mut Citrine,
        ctx: &Context,
        events: Vec<Event>,
        width: f32,
        height: f32,
    ) -> egui::FullOutput {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, height))),
            events,
            max_texture_side: Some(8192),
            ..Default::default()
        };
        ctx.run(input, |ctx| {
            app.render(ctx);
            app.handle_event_queue(false);
        })
    }

    fn press(at: Pos2, pressed: bool) -> Vec<Event> {
        vec![
            Event::PointerMoved(at),
            Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            },
        ]
    }

    #[test]
    fn picking_a_search_hit_opens_the_page_and_scrolls_to_the_row() {
        let mut app = phone_app("settings-search-pick");
        app.ui.settings.layout = LayoutMode::Desktop;
        app.ui.settings.focus_search = true;
        app.open_tab(Tab::Settings);
        let ctx = Context::default();
        for _ in 0..3 {
            frame(&mut app, &ctx, Vec::new());
        }
        frame(&mut app, &ctx, vec![Event::Text("updates".to_owned())]);
        assert_eq!(app.ui.settings.search, "updates");
        let label = RowId::CheckUpdates.label(Surface::Desktop);
        let output = frame(&mut app, &ctx, Vec::new());
        let hit = text_rect(&output, label).expect("the hit for the update row");
        frame(&mut app, &ctx, press(hit.center(), true));
        frame(&mut app, &ctx, press(hit.center(), false));
        assert_eq!(app.ui.settings.current_tab, RowId::CheckUpdates.page());
        assert!(app.ui.settings.search.is_empty());
        let mut output = frame(&mut app, &ctx, Vec::new());
        for _ in 0..30 {
            output = frame(&mut app, &ctx, Vec::new());
        }
        assert!(app.ui.settings.find.is_none());
        let row = text_rect(&output, label).expect("the update row on its page");
        assert!(row.min.y >= 0.0 && row.max.y <= HEIGHT, "{row:?}");
    }

    fn glows(ctx: &Context, output: &egui::FullOutput, row: Rect) -> bool {
        let glow = ctx.style().visuals.selection.bg_fill;
        let tint = |c: egui::Color32| {
            let a = c.a().max(1) as f32;
            [c.r() as f32 / a, c.g() as f32 / a, c.b() as f32 / a]
        };
        output.shapes.iter().any(|clipped| match &clipped.shape {
            egui::Shape::Rect(rect) => {
                let fill = rect.fill;
                fill.a() > 0
                    && fill.a() < glow.a()
                    && tint(fill)
                        .iter()
                        .zip(tint(glow))
                        .all(|(have, want)| (have - want).abs() < 0.1)
                    && rect.rect.y_range().contains(row.center().y)
                    && rect.rect.width() >= row.width()
            }
            _ => false,
        })
    }

    #[test]
    fn a_picked_hit_glows_on_its_page_beside_the_list_and_in_a_narrow_pane() {
        for narrow in [false, true] {
            let mut app = phone_app("settings-search-glow");
            app.ui.settings.layout = LayoutMode::Desktop;
            if narrow {
                app.dock = DockState::new(vec![Tab::GameBoy]);
                app.dock.main_surface_mut().split_right(
                    NodeIndex::root(),
                    0.7,
                    vec![Tab::Settings],
                );
            }
            app.ui.settings.focus_search = true;
            app.open_tab(Tab::Settings);
            let ctx = Context::default();
            for _ in 0..3 {
                frame(&mut app, &ctx, Vec::new());
            }
            assert_eq!(app.ui.settings_list_open, narrow);
            frame(&mut app, &ctx, vec![Event::Text("fast".to_owned())]);
            let label = RowId::FastForward.label(Surface::Desktop);
            let output = frame(&mut app, &ctx, Vec::new());
            let hit = text_rect(&output, label).expect("the hit for fast forward");
            frame(&mut app, &ctx, press(hit.center(), true));
            frame(&mut app, &ctx, press(hit.center(), false));
            assert_eq!(app.ui.settings.current_tab, RowId::FastForward.page());
            let mut glowed = false;
            for _ in 0..10 {
                let output = frame(&mut app, &ctx, Vec::new());
                if let Some(row) = text_rect(&output, label) {
                    glowed |= glows(&ctx, &output, row);
                }
            }
            assert!(glowed, "no glow on the fast forward row, narrow {narrow}");
        }
    }

    fn key(key: egui::Key) -> Vec<Event> {
        vec![Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }]
    }

    #[test]
    fn tab_then_enter_from_the_search_opens_the_first_hit_beside_the_list() {
        let mut app = phone_app("settings-search-tab");
        app.ui.settings.layout = LayoutMode::Desktop;
        app.ui.settings.focus_search = true;
        app.open_tab(Tab::Settings);
        let ctx = Context::default();
        for _ in 0..3 {
            frame(&mut app, &ctx, Vec::new());
        }
        assert!(!app.ui.settings_list_open);
        frame(&mut app, &ctx, vec![Event::Text("fast".to_owned())]);
        frame(&mut app, &ctx, Vec::new());
        frame(&mut app, &ctx, key(egui::Key::Tab));
        frame(&mut app, &ctx, Vec::new());
        frame(&mut app, &ctx, key(egui::Key::Enter));
        for _ in 0..3 {
            frame(&mut app, &ctx, Vec::new());
        }
        assert_eq!(app.ui.settings.current_tab, RowId::FastForward.page());
        assert!(app.ui.settings.search.is_empty());
    }

    fn porklike() -> Vec<u8> {
        let mut rom = vec![0u8; 0x8000];
        rom[0x134..0x13C].copy_from_slice(b"PORKLIKE");
        rom
    }

    fn count(output: &egui::FullOutput, wanted: &str) -> usize {
        output
            .shapes
            .iter()
            .filter(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => text.galley.text().trim() == wanted,
                _ => false,
            })
            .count()
    }

    fn texts(output: &egui::FullOutput) -> Vec<String> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => Some(text.galley.text().trim().to_owned()),
                _ => None,
            })
            .filter(|text| !text.is_empty())
            .collect()
    }

    #[test]
    fn a_running_game_lets_the_desktop_page_switch_between_all_games_and_the_game() {
        use crate::app::tests::load_game;
        use crate::app::ui_state::per_game::Scope;
        use crate::app::ui_state::settings_tree::SettingsTab;
        use crate::app::widgets::settings::scope::{self, ALL_GAMES, MARK};
        let mut app = phone_app("settings-scope-bar");
        app.ui.settings.layout = LayoutMode::Desktop;
        app.ui.settings.current_tab = SettingsTab::Screen;
        app.open_tab(Tab::Settings);
        let ctx = Context::default();
        let mut output = frame(&mut app, &ctx, Vec::new());
        for _ in 0..3 {
            output = frame(&mut app, &ctx, Vec::new());
        }
        assert_eq!(count(&output, ALL_GAMES), 0, "no game, no scope bar");

        load_game(&mut app, &porklike(), None);
        app.ui.settings.play_game(app.emulator.rom_key());
        for _ in 0..3 {
            output = frame(&mut app, &ctx, Vec::new());
        }
        assert!(app.ui.settings.playing_key().is_some(), "the game plays");
        assert!(
            scope::shows_bar(&app.ui.settings, SettingsTab::Screen, Surface::Desktop),
            "the screen page has per-game rows"
        );
        assert!(
            app.dock.find_tab(&Tab::Settings).is_some(),
            "settings stays open"
        );
        assert_eq!(count(&output, ALL_GAMES), 1, "{:?}", texts(&output));
        let match_refresh = RowId::MatchRefresh.label(Surface::Desktop);
        assert!(text_rect(&output, match_refresh).is_some());
        let segment = format!("{MARK} {}", scope::game_title(&app.ui.settings));
        let game = text_rect(&output, &segment).expect("the game's side of the bar");
        frame(&mut app, &ctx, press(game.center(), true));
        frame(&mut app, &ctx, press(game.center(), false));
        assert_eq!(app.ui.settings.scope(), Scope::ThisGame);
        for _ in 0..3 {
            output = frame(&mut app, &ctx, Vec::new());
        }
        assert!(
            count(&output, ALL_GAMES) > 1,
            "rows that follow all games say so"
        );
        assert!(
            text_rect(&output, match_refresh).is_none(),
            "a row that can't differ per game hides"
        );

        frame(&mut app, &ctx, key(egui::Key::Escape));
        assert_eq!(app.ui.settings.scope(), Scope::AllGames);
        assert_eq!(app.ui.settings.current_tab, SettingsTab::Screen);

        app.ui.settings.current_tab = SettingsTab::App;
        for _ in 0..3 {
            output = frame(&mut app, &ctx, Vec::new());
        }
        assert_eq!(count(&output, ALL_GAMES), 0, "a page without per-game rows");
    }

    fn galley_at(output: &egui::FullOutput, wanted: &str) -> Option<(Rect, usize)> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text().trim() == wanted => {
                    Some((text.visual_bounding_rect(), text.galley.rows.len()))
                }
                _ => None,
            })
    }

    fn texts_beside(output: &egui::FullOutput, row: Rect) -> Vec<(String, Rect)> {
        let mut found: Vec<(String, Rect)> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => {
                    let rect = text.visual_bounding_rect();
                    let words = text.galley.text().trim().to_owned();
                    ((rect.center().y - row.center().y).abs() < 8.0
                        && rect.left() > row.right()
                        && words != crate::icons::INFO)
                        .then_some((words, rect))
                }
                _ => None,
            })
            .collect();
        found.sort_by(|a, b| a.1.left().total_cmp(&b.1.left()));
        found
    }

    fn game_boy_page_with_own_palette(name: &str) -> (Citrine, Context) {
        use crate::app::tests::load_game;
        use crate::app::ui_state::settings_tree::SettingsTab;
        use citrine_gb::gb::ppu::types::theme::DmgTheme;
        let mut app = phone_app(name);
        app.ui.settings.layout = LayoutMode::Desktop;
        app.ui.settings.current_tab = SettingsTab::GameBoy;
        app.open_tab(Tab::Settings);
        let ctx = Context::default();
        for _ in 0..3 {
            tall_frame(&mut app, &ctx, Vec::new(), 900.0);
        }
        load_game(&mut app, &porklike(), None);
        app.ui.settings.play_game(app.emulator.rom_key());
        let key = app.emulator.rom_key().expect("key").to_owned();
        app.ui.settings.set_own(
            &key,
            "dmg_theme",
            serde_json::to_value(DmgTheme::GreyScale).expect("theme"),
        );
        (app, ctx)
    }

    fn settle(app: &mut Citrine, ctx: &Context, width: f32) -> egui::FullOutput {
        let mut output = sized_frame(app, ctx, Vec::new(), width, 900.0);
        for _ in 0..4 {
            output = sized_frame(app, ctx, Vec::new(), width, 900.0);
        }
        output
    }

    #[test]
    fn the_game_scope_keeps_the_label_column_and_drops_the_reset_arrow() {
        use crate::app::ui_state::per_game::Scope;
        let (mut app, ctx) = game_boy_page_with_own_palette("settings-scope-columns");
        for scope in [Scope::AllGames, Scope::ThisGame] {
            app.ui.settings.set_scope(scope);
            let output = settle(&mut app, &ctx, 1280.0);
            for row in [RowId::GbcScreen, RowId::Palette, RowId::GbcBootRom] {
                let label = row.label(Surface::Desktop);
                if scope == Scope::ThisGame && !row.per_game() {
                    continue;
                }
                let (_, lines) = galley_at(&output, label)
                    .unwrap_or_else(|| panic!("{scope:?}: no {label} in {:?}", texts(&output)));
                assert_eq!(lines, 1, "{scope:?}: {label} wraps");
            }
            let arrows = count(&output, crate::icons::ARROW_COUNTER_CLOCKWISE);
            if scope == Scope::ThisGame {
                assert_eq!(arrows, 0, "the tag's x is the one way back");
            } else {
                assert!(arrows > 0, "all games keeps its reset arrows");
            }
        }
    }

    #[test]
    fn the_screen_page_keeps_one_label_column_while_the_game_owns_values() {
        use crate::app::ui_state::per_game::Scope;
        use crate::app::ui_state::settings_tree::SettingsTab;
        let (mut app, ctx) = game_boy_page_with_own_palette("settings-screen-one-line");
        app.ui.settings.current_tab = SettingsTab::Screen;
        app.ui.settings.ghosting = true;
        app.ui.settings.matrix = false;
        let key = app.emulator.rom_key().expect("key").to_owned();
        app.ui
            .settings
            .set_own(&key, "matrix", serde_json::json!(true));
        app.ui
            .settings
            .set_own(&key, "ghosting_strength", serde_json::json!(0.8));
        for scope in [Scope::AllGames, Scope::ThisGame] {
            app.ui.settings.set_scope(scope);
            let output = settle(&mut app, &ctx, 1280.0);
            let label = RowId::GhostingStrength.label(Surface::Desktop);
            let (_, lines) = galley_at(&output, label)
                .unwrap_or_else(|| panic!("{scope:?}: no {label} in {:?}", texts(&output)));
            assert_eq!(lines, 1, "{scope:?}: {label} wraps");
        }
    }

    #[test]
    fn a_short_row_keeps_the_all_games_slider_length_with_short_chips() {
        use crate::app::ui_state::per_game::Scope;
        use crate::app::ui_state::settings_tree::SettingsTab;
        use crate::app::widgets::settings::scope;
        use crate::icons;
        let (mut app, ctx) = game_boy_page_with_own_palette("settings-short-chips");
        app.ui.settings.current_tab = SettingsTab::Screen;
        app.ui.settings.ghosting = true;
        app.ui.settings.matrix = false;
        let key = app.emulator.rom_key().expect("key").to_owned();
        app.ui
            .settings
            .set_own(&key, "ghosting_strength", serde_json::json!(0.8));
        let label = RowId::GhostingStrength.label(Surface::Desktop);
        let mut value_x = Vec::new();
        for (at, value) in [(Scope::AllGames, "30%"), (Scope::ThisGame, "80%")] {
            app.ui.settings.set_scope(at);
            let output = settle(&mut app, &ctx, 1280.0);
            let (row, _) = galley_at(&output, label).expect("label");
            let beside = texts_beside(&output, row);
            let (_, rect) = beside
                .iter()
                .find(|(words, _)| words == value)
                .unwrap_or_else(|| panic!("{at:?}: no {value} in {beside:?}"));
            value_x.push(rect.left());
            if at == Scope::ThisGame {
                let short = format!("{} {}", scope::MARK, icons::X);
                assert!(
                    beside.iter().any(|(words, _)| *words == short),
                    "no short chip in {beside:?}"
                );
                for follows in [RowId::LcdGrid, RowId::Ghosting] {
                    let (row, _) = galley_at(&output, follows.label(Surface::Desktop))
                        .unwrap_or_else(|| panic!("no {follows:?} in {:?}", texts(&output)));
                    let beside = texts_beside(&output, row);
                    assert!(
                        !beside.iter().any(|(words, _)| words == scope::ALL_GAMES),
                        "{follows:?}: {beside:?}"
                    );
                }
            }
        }
        assert!(value_x[1] + 0.5 >= value_x[0], "{value_x:?}");
    }

    #[test]
    fn the_controls_stay_in_place_on_a_scope_switch_once_the_page_has_room() {
        use crate::app::ui_state::per_game::Scope;
        let (mut app, ctx) = game_boy_page_with_own_palette("settings-scope-still");
        let label = RowId::GbcScreen.label(Surface::Desktop);
        for width in [1280.0, 1408.0] {
            let mut controls = Vec::new();
            for scope in [Scope::AllGames, Scope::ThisGame, Scope::AllGames] {
                app.ui.settings.set_scope(scope);
                let output = settle(&mut app, &ctx, width);
                let (row, _) = galley_at(&output, label)
                    .unwrap_or_else(|| panic!("{scope:?}: no {label} in {:?}", texts(&output)));
                let beside = texts_beside(&output, row);
                let (value, rect) = beside.first().expect("its select");
                if width > 1400.0 {
                    assert!(!value.ends_with('\u{2026}'), "{scope:?}: {value} is cut");
                }
                controls.push(rect.left());
            }
            assert!(
                controls.iter().all(|x| (x - controls[0]).abs() < 0.5),
                "switching scope moves the controls at {width}: {controls:?}"
            );
        }
    }

    #[test]
    fn the_all_games_tag_is_the_mark_alone() {
        use crate::app::widgets::settings::scope::MARK;
        let (mut app, ctx) = game_boy_page_with_own_palette("settings-scope-mark");
        let output = settle(&mut app, &ctx, 1280.0);
        let label = RowId::Palette.label(Surface::Desktop);
        let (row, _) = galley_at(&output, label)
            .unwrap_or_else(|| panic!("no {label} in {:?}", texts(&output)));
        let beside: Vec<String> = texts_beside(&output, row)
            .into_iter()
            .map(|(text, _)| text)
            .collect();
        assert_eq!(beside.last().map(String::as_str), Some(MARK), "{beside:?}");
        assert!(
            !texts(&output)
                .iter()
                .any(|text| text.contains("Grey Scale")),
            "the game's value shows only on hover"
        );
    }

    #[test]
    fn the_big_screen_mark_is_the_pin_and_value_without_the_game_name() {
        use crate::app::tests::load_game;
        use crate::app::ui_state::settings_tree::SettingsTab;
        use crate::app::widgets::settings::scope::{self, MARK};
        let mut app = phone_app("settings-scope-big-mark");
        app.ui.settings.big_screen.on = true;
        app.ui.settings.ghosting = true;
        let ctx = Context::default();
        load_game(&mut app, &porklike(), None);
        app.ui.settings.play_game(app.emulator.rom_key());
        let key = app.emulator.rom_key().expect("key").to_owned();
        app.ui
            .settings
            .set_own(&key, "ghosting_strength", serde_json::json!(0.8));
        app.open_settings(SettingsTab::Screen, true);
        let mut output = sized_frame(&mut app, &ctx, Vec::new(), 1280.0, 720.0);
        for _ in 0..4 {
            output = sized_frame(&mut app, &ctx, Vec::new(), 1280.0, 720.0);
        }
        let game = scope::game_title(&app.ui.settings).to_owned();
        let bar = format!("{MARK} {game}");
        let shown = texts(&output);
        assert!(
            shown
                .iter()
                .any(|text| text.trim() == format!("{MARK} 80%")),
            "{shown:?}"
        );
        assert!(
            shown
                .iter()
                .all(|text| text.trim() == bar || !text.contains(&game)),
            "{shown:?}"
        );
    }

    fn box_around(output: &egui::FullOutput, at: Pos2) -> Option<Rect> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape) if shape.rect.contains(at) => Some(shape.rect),
                _ => None,
            })
            .min_by(|a, b| a.area().total_cmp(&b.area()))
    }

    #[test]
    fn the_selects_share_one_column_and_the_mark_sits_after_the_reset_arrows() {
        use crate::app::widgets::settings::scope::MARK;
        let (mut app, ctx) = game_boy_page_with_own_palette("settings-scope-select-column");
        let output = settle(&mut app, &ctx, 1280.0);
        let mut rows = Vec::new();
        for row in [RowId::Palette, RowId::GbcBootRom, RowId::GbcScreen] {
            let label = row.label(Surface::Desktop);
            let (at, _) = galley_at(&output, label)
                .unwrap_or_else(|| panic!("no {label} in {:?}", texts(&output)));
            let beside = texts_beside(&output, at);
            let (value, value_at) = beside.first().expect("its select");
            let select = box_around(&output, value_at.center())
                .unwrap_or_else(|| panic!("{label}: no box around {value}"));
            let (_, arrow) = beside
                .iter()
                .find(|(text, _)| text == crate::icons::ARROW_COUNTER_CLOCKWISE)
                .unwrap_or_else(|| panic!("{label}: no reset arrow in {beside:?}"));
            rows.push((label, select, *arrow, beside));
        }
        let (_, first, first_arrow, _) = &rows[0];
        for (label, select, arrow, _) in &rows {
            assert!(
                (select.left() - first.left()).abs() < 0.5
                    && (select.width() - first.width()).abs() < 0.5,
                "{label}: select {select:?}, Palette's {first:?}"
            );
            assert!(
                (arrow.center().x - first_arrow.center().x).abs() < 0.5,
                "{label}: reset arrow at {arrow:?}, Palette's at {first_arrow:?}"
            );
        }
        let (_, select, arrow, beside) = &rows[0];
        let (mark, mark_at) = beside.last().expect("the mark");
        assert_eq!(mark, MARK, "{beside:?}");
        let chip = box_around(&output, mark_at.center()).expect("the mark's chip");
        assert!(chip.left() > arrow.right(), "{chip:?} overlaps {arrow:?}");
        assert!(
            (chip.center().y - select.center().y).abs() < 0.5,
            "the mark at {chip:?} is off the select's middle {select:?}"
        );
    }

    #[test]
    fn a_cut_select_value_ends_on_a_letter() {
        let (mut app, ctx) = game_boy_page_with_own_palette("settings-scope-cut");
        app.ui
            .settings
            .set_scope(crate::app::ui_state::per_game::Scope::ThisGame);
        let output = settle(&mut app, &ctx, 1280.0);
        let cut: Vec<String> = texts(&output)
            .into_iter()
            .filter(|text| text.ends_with('\u{2026}'))
            .collect();
        assert!(!cut.is_empty(), "at 1280 some values are cut");
        assert!(
            cut.iter()
                .all(|text| !text.trim_end_matches('\u{2026}').ends_with(' ')),
            "{cut:?}"
        );
    }

    #[test]
    fn a_long_game_name_takes_one_line_in_the_desktop_scope_bar() {
        use crate::app::tests::load_game;
        use crate::app::ui_state::settings_tree::SettingsTab;
        use crate::app::widgets::settings::scope::MARK;
        let mut app = phone_app("settings-scope-long-name");
        app.ui.settings.layout = LayoutMode::Desktop;
        app.ui.settings.current_tab = SettingsTab::Screen;
        app.open_tab(Tab::Settings);
        let ctx = Context::default();
        for _ in 0..3 {
            tall_frame(&mut app, &ctx, Vec::new(), 900.0);
        }
        load_game(&mut app, &porklike(), None);
        app.emulator.name_game(Some(
            "The Legend of a Very Long Game Name That Goes On and On (Europe) (En,Fr,De).gb",
        ));
        app.ui.settings.name_game(app.emulator.game_name());
        app.ui.settings.play_game(app.emulator.rom_key());
        let mut output = tall_frame(&mut app, &ctx, Vec::new(), 900.0);
        for _ in 0..4 {
            output = tall_frame(&mut app, &ctx, Vec::new(), 900.0);
        }
        let segment = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text)
                    if text.galley.text().starts_with(MARK)
                        && text.galley.text().contains("Legend") =>
                {
                    Some((
                        text.galley.text().to_owned(),
                        text.galley.rows.len(),
                        text.galley.elided,
                    ))
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("no game side in {:?}", texts(&output)));
        assert_eq!(segment.1, 1, "{segment:?}");
        assert!(segment.2, "the name shortens: {segment:?}");
    }

    #[test]
    fn finding_a_setting_that_cannot_differ_per_game_leaves_the_game_scope() {
        use crate::app::tests::load_game;
        use crate::app::ui_state::per_game::Scope;
        use crate::app::ui_state::settings_tree::GroupId;
        let mut app = phone_app("settings-scope-find");
        load_game(&mut app, &porklike(), None);
        app.ui.settings.play_game(app.emulator.rom_key());
        app.ui.settings.set_scope(Scope::ThisGame);
        app.find_setting(RowId::LcdGrid, true);
        assert_eq!(app.ui.settings.scope(), Scope::ThisGame);
        app.find_setting(RowId::ShowHuds, true);
        assert_eq!(app.ui.settings.scope(), Scope::AllGames);
        app.ui.settings.set_scope(Scope::ThisGame);
        app.find_group(GroupId::Effects, true);
        assert_eq!(app.ui.settings.scope(), Scope::ThisGame);
        app.find_group(GroupId::Huds, true);
        assert_eq!(app.ui.settings.scope(), Scope::AllGames);
    }

    #[test]
    fn a_running_game_lets_the_phone_page_switch_and_back_steps_out_first() {
        use crate::app::tests::{load_game, play_frame, tap, text_containing};
        use crate::app::ui_state::per_game::Scope;
        use crate::app::ui_state::settings_tree::SettingsTab;
        use crate::app::widgets::settings::scope::{self, ALL_GAMES, MARK};
        let mut app = phone_app("settings-scope-phone");
        load_game(&mut app, &porklike(), None);
        app.ui.settings.play_game(app.emulator.rom_key());
        app.mobile.nav.open_settings(SettingsTab::Screen);
        let ctx = Context::default();
        let mut output = play_frame(&mut app, &ctx, Vec::new());
        for _ in 0..3 {
            output = play_frame(&mut app, &ctx, Vec::new());
        }
        assert!(app.ui.settings.playing_key().is_some(), "the game plays");
        assert_eq!(
            app.mobile.nav.settings_page(),
            Some(SettingsTab::Screen),
            "the screen page stays open"
        );
        assert!(
            scope::shows_bar(&app.ui.settings, SettingsTab::Screen, Surface::Phone),
            "the screen page has per-game rows"
        );
        assert_eq!(count(&output, ALL_GAMES), 1, "{:?}", texts(&output));
        let game = text_containing(&output, MARK).expect("the game's segment");
        play_frame(&mut app, &ctx, tap(game.center()));
        for _ in 0..3 {
            output = play_frame(&mut app, &ctx, Vec::new());
        }
        assert_eq!(app.ui.settings.scope(), Scope::ThisGame);
        assert!(count(&output, ALL_GAMES) > 1);

        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(412.0, 915.0))),
            events: key(egui::Key::Escape),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            app.render(ctx);
            app.mobile_escape(ctx);
        });
        play_frame(&mut app, &ctx, Vec::new());
        assert_eq!(app.ui.settings.scope(), Scope::AllGames);
        assert_eq!(app.mobile.nav.settings_page(), Some(SettingsTab::Screen));
    }
}

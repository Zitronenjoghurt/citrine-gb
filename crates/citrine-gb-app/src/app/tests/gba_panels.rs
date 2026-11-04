use super::*;

const SPIN: [u8; 4] = 0xEAFF_FFFE_u32.to_le_bytes();

fn gba_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x400];
    rom[..4].copy_from_slice(&SPIN);
    rom
}

fn both_consoles_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x0143] = 0x80;
    rom
}

fn app_on(name: &str, layout: LayoutMode, gba: bool) -> Citrine {
    let mut app = phone_app(name);
    app.ui.settings.layout = layout;
    app.ui.settings.dev_mode = true;
    if gba {
        app.emulator.put_gba(&gba_rom(), &format!("gba-{name}"));
    } else {
        load_game(&mut app, &both_consoles_rom(), None);
        app.emulator.running = true;
    }
    app
}

fn frames(app: &mut Citrine, ctx: &Context, width: f32) -> egui::FullOutput {
    info_frame_out(app, ctx, width, 900.0);
    info_frame_out(app, ctx, width, 900.0)
}

fn shows(output: &egui::FullOutput, text: &str) -> bool {
    text_rect(output, text).is_some() || text_containing(output, text).is_some()
}

fn desktop_tab(name: &str, gba: bool, tab: Tab) -> egui::FullOutput {
    let mut app = app_on(name, LayoutMode::Desktop, gba);
    app.open_tab(tab);
    frames(&mut app, &Context::default(), 1280.0)
}

#[test]
fn a_gba_hides_the_palettes_in_settings() {
    for gba in [false, true] {
        let mut app = app_on(&format!("palettes-{gba}"), LayoutMode::Desktop, gba);
        app.ui.settings.current_tab = SettingsTab::Style;
        app.open_tab(Tab::Settings);
        let output = frames(&mut app, &Context::default(), 1280.0);
        assert_eq!(
            text_rect(&output, "Palette").is_some(),
            !gba,
            "desktop, gba {gba}"
        );
        assert!(shows(&output, "Scaling"), "desktop, gba {gba}");

        let mut phone = app_on(&format!("phone-palettes-{gba}"), LayoutMode::Mobile, gba);
        phone.mobile.nav.open_settings(SettingsTab::Style);
        let output = frames(&mut phone, &Context::default(), 412.0);
        assert_eq!(shows(&output, "New palette"), !gba, "phone, gba {gba}");
        assert!(shows(&output, "Scaling"), "phone, gba {gba}");
    }
}

#[test]
fn a_gba_hides_the_json_dump_in_debug_actions() {
    for gba in [false, true] {
        let output = desktop_tab(&format!("dump-{gba}"), gba, Tab::DebugActions);
        assert_eq!(shows(&output, "Export as JSON"), !gba, "gba {gba}");
        assert!(shows(&output, "Input recording"), "gba {gba}");
    }
}

#[test]
fn a_gba_offers_no_other_console() {
    for gba in [false, true] {
        let mut phone = app_on(&format!("play-on-{gba}"), LayoutMode::Mobile, gba);
        phone.mobile.nav.show_game();
        phone.mobile.nav.set_more(true);
        let output = frames(&mut phone, &Context::default(), 412.0);
        assert_eq!(shows(&output, "Play on"), !gba, "more, gba {gba}");

        let mut phone = app_on(&format!("console-{gba}"), LayoutMode::Mobile, gba);
        phone.mobile.nav.open_settings(SettingsTab::General);
        let output = frames(&mut phone, &Context::default(), 412.0);
        assert_eq!(
            shows(&output, "This game's console"),
            !gba,
            "general, gba {gba}"
        );
    }
}

#[test]
fn a_gba_runs_through_every_dev_tool_and_console_panel() {
    let tools = crate::app::widgets::dev_menu::DEV_SECTIONS
        .iter()
        .flat_map(|section| section.tools)
        .map(|(_, tab)| *tab);
    let panels = [
        Tab::GameBoy,
        Tab::RomInfo,
        Tab::Settings,
        Tab::GameInfo,
        Tab::Saves,
    ];
    for tab in tools.chain(panels) {
        for width in [412.0, 1280.0] {
            let layout = if width < 500.0 {
                LayoutMode::Mobile
            } else {
                LayoutMode::Desktop
            };
            let mut app = app_on(&format!("tool-{tab:?}-{width}"), layout, true);
            app.open_tab(tab);
            app.mobile.open(tab);
            let ctx = Context::default();
            for _ in 0..3 {
                let _ = play_frame(&mut app, &ctx, Vec::new());
                info_frame_out(&mut app, &ctx, width, 900.0);
            }
            assert!(app.emulator.as_gb().is_none(), "{tab:?}");
        }
    }
}

#[test]
fn a_gba_autosave_row_keeps_delete_inside_the_dock() {
    let mut app = app_on("autosave-delete", LayoutMode::Desktop, true);
    app.emulator.autosave().unwrap();
    app.emulator.save_snapshot(0).unwrap();
    app.open_tab(Tab::Saves);
    let web_1280_css = 1280.0 / 2.2;
    let output = frames(&mut app, &Context::default(), web_1280_css);
    let deletes: Vec<(egui::Rect, egui::Rect)> = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.text().contains("Delete") => {
                Some((text.visual_bounding_rect(), clipped.clip_rect))
            }
            _ => None,
        })
        .collect();
    assert!(!deletes.is_empty(), "no Delete button in the Saves tab");
    for (text, clip) in deletes {
        assert!(
            text.max.x <= clip.max.x,
            "Delete ends at {} past the dock edge {}",
            text.max.x,
            clip.max.x
        );
    }
}

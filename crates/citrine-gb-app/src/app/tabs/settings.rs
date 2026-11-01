use crate::app::tabs::TabViewer;
use crate::app::ui_state::settings::SettingsTab;
use crate::app::ui_state::settings::rom_header;
use crate::app::widgets::settings::bar::SettingsBar;
use crate::app::widgets::settings::content::{LoadedGame, SettingsContent};
use crate::app::widgets::tab_bar::tab_bar;
use egui::Widget;

const SIDEBAR_MIN_WIDTH: f32 = 420.0;

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    if ui.available_width() < SIDEBAR_MIN_WIDTH {
        stacked(viewer, ui);
    } else {
        side_by_side(viewer, ui);
    }
}

fn side_by_side(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    let width = ui.available_width();
    let height = ui.available_height();
    let tabs_width = (width * 0.2).max(120.0);

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(tabs_width);
            ui.set_height(height);
            SettingsBar::new(&mut viewer.ui.settings)
                .spacing(2.0)
                .ui(ui);
        });

        ui.separator();

        ui.vertical_centered(|ui| content(viewer, ui, 0.0));
    });
}

fn stacked(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    let settings = &mut viewer.ui.settings;
    let dev_mode = settings.dev_mode;
    let docked = !crate::app::mobile_layout(settings.layout, ui.ctx());

    let tabs: Vec<(SettingsTab, String)> = SettingsTab::iter_with_dev_mode(dev_mode)
        .map(|tab| (tab, format!("{} {}", tab.icon(), tab.title())))
        .collect();
    tab_bar(ui, &mut settings.current_tab, &tabs);
    let line = ui.separator().rect.center().y;
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

fn content(viewer: &mut TabViewer, ui: &mut egui::Ui, top_space: f32) {
    let gb = &viewer.emulator.gb;
    let header = rom_header(gb);
    let game = viewer.emulator.rom_key().map(|rom_key| LoadedGame {
        rom_key,
        title: viewer.emulator.game_name(),
        model: viewer.emulator.console(),
        cgb_mode: gb.cartridge.header.cgb_mode,
        header: &header,
        own_colors: gb.ppu.cgb_mode(),
    });

    #[cfg(not(target_arch = "wasm32"))]
    let discord = (
        crate::discord::game_now(viewer.emulator, viewer.stats, viewer.game_info),
        viewer.emulator.screen_texture(),
        viewer.online.pictures_off(),
    );
    let content = SettingsContent::new(
        &mut viewer.ui.settings,
        &mut viewer.ui.capture,
        &mut viewer.ui.palette_editor,
        viewer.audio.as_ref(),
        game,
        Some(viewer.events),
    )
    .online(viewer.online, &mut viewer.ui.join)
    .your_data(&mut viewer.ui.your_data)
    .top_space(top_space);
    #[cfg(not(target_arch = "wasm32"))]
    let content = content.discord(discord.0, discord.1, discord.2);
    content.ui(ui);
}

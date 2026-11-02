use crate::app::events::AppEventQueue;
use crate::app::tabs::TabViewer;
use crate::app::ui_state::settings::{SettingsTab, console_name};
use crate::app::widgets::action::wide_button;
use crate::app::widgets::homebrew::{self, HomebrewList};
use crate::emulator::Emulator;
use crate::icons;
use crate::local_link::{Controls, Game, Half, LocalLink};
use crate::utils::file_loader::FileLoader;
use citrine_gb::gb::GameBoy;
use egui::{Label, RichText, Ui};
use strum::IntoEnumIterator;

pub mod partner;

pub const LINK_ICON: &str = icons::PLUGS_CONNECTED;
const SCREEN_WIDTH: usize = 160;
const SCREEN_HEIGHT: usize = 144;
const MAX_SCREEN_SCALE: f32 = 3.0;
const MIN_SCREEN_HEIGHT: f32 = SCREEN_HEIGHT as f32 / 2.0;
const ROW_HEIGHT: f32 = 36.0;

#[derive(Default)]
pub struct LinkUi {
    screen: Option<egui::TextureHandle>,
    painted_pass: u64,
    screen_height: f32,
    fitted_height: Option<f32>,
    pub picking: bool,
    pub listing: bool,
    pub from_list: bool,
    pub events: partner::EventCache,
}

pub fn title(emulator: &Emulator, listing: bool) -> String {
    match &emulator.link {
        Some(link) => link.title().to_owned(),
        None if listing || emulator.partner.is_some() => "Event partner".to_owned(),
        None => "Link a second game".to_owned(),
    }
}

pub fn driver(link: &LocalLink, game: Game, touch: bool) -> String {
    let icon = |side: &str| {
        if side.starts_with("Controller") {
            icons::GAME_CONTROLLER
        } else {
            icons::KEYBOARD
        }
    };
    match (link.driver(game), touch) {
        (Some("Playing here"), _) => format!("{}  Playing here", icons::GAME_CONTROLLER),
        (Some(side), _) => {
            let settings = link.settings();
            if settings.controls == Controls::SplitKeyboard {
                let half = if settings.game_of_side(0) == game {
                    Half::Left
                } else {
                    Half::Right
                };
                format!("{}  {}", icons::KEYBOARD, settings.key_names(half))
            } else {
                format!("{}  {side}", icon(side))
            }
        }
        (None, true) => format!("{}  Tap to play here", icons::HAND_TAP),
        (None, false) => format!("{}  Click to play here", icons::CURSOR_CLICK),
    }
}

fn texture(link_ui: &mut LinkUi, ctx: &egui::Context, gb: &GameBoy) -> egui::TextureId {
    let pass = ctx.cumulative_pass_nr();
    if link_ui.painted_pass != pass || link_ui.screen.is_none() {
        link_ui.painted_pass = pass;
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [SCREEN_WIDTH, SCREEN_HEIGHT],
            gb.frame().as_slice(),
        );
        match &mut link_ui.screen {
            Some(texture) => texture.set(image, egui::TextureOptions::NEAREST),
            None => {
                link_ui.screen =
                    Some(ctx.load_texture("second_screen", image, egui::TextureOptions::NEAREST));
            }
        }
    }
    link_ui
        .screen
        .as_ref()
        .map_or(egui::TextureId::default(), |t| t.id())
}

pub fn paint_screen(
    link_ui: &mut LinkUi,
    ui: &mut Ui,
    gb: &GameBoy,
    area: egui::Rect,
    max_scale: f32,
) -> egui::Rect {
    let texture = texture(link_ui, ui.ctx(), gb);
    let rect = fit(area, max_scale);
    egui::Image::new(egui::load::SizedTexture::new(texture, rect.size())).paint_at(ui, rect);
    rect
}

pub fn fit(area: egui::Rect, max_scale: f32) -> egui::Rect {
    let scale = (area.width() / SCREEN_WIDTH as f32)
        .min(area.height() / SCREEN_HEIGHT as f32)
        .min(max_scale);
    let size = egui::vec2(SCREEN_WIDTH as f32, SCREEN_HEIGHT as f32) * scale;
    let min = area.center() - size / 2.0;
    egui::Rect::from_min_size(min.round(), size.round())
}

pub fn show(viewer: &mut TabViewer, ui: &mut Ui) {
    if viewer.emulator.link.is_some() {
        panel(viewer, ui);
    } else if viewer.ui.linked.listing {
        partner::list(viewer, ui);
    } else if viewer.emulator.partner.is_some() {
        partner::panel(viewer, ui);
    } else {
        shelf(viewer, ui);
    }
}

fn shelf(viewer: &mut TabViewer, ui: &mut Ui) {
    let size = egui::vec2(ui.available_width(), ROW_HEIGHT);
    if ui
        .add_sized(
            size,
            egui::Button::new(format!("{}  Open a ROM file", icons::FOLDER_OPEN)),
        )
        .clicked()
    {
        FileLoader::new()
            .title("Link a second game")
            .add_filter("Game Boy ROMs", &["gb", "gbc"])
            .dispatch(viewer.files.second_rom_tx.clone());
    }
    ui.add_space(8.0);
    let partnered = partner::has_events(viewer, ui.ctx());
    let labels: &[&str] = if partnered {
        &["Link", "Open"]
    } else {
        &["Link"]
    };
    let button_width = homebrew::action_width(ui, labels);
    if partnered {
        partner::shelf_row(viewer, ui, button_width);
        ui.add_space(6.0);
    }
    egui::ScrollArea::vertical()
        .auto_shrink([false, true])
        .show(ui, |ui| {
            #[cfg(not(target_arch = "wasm32"))]
            let titled = recent_rows(viewer, ui, button_width) || partnered;
            #[cfg(target_arch = "wasm32")]
            let titled = partnered;
            if titled {
                heading(ui, "Homebrew games");
            }
            HomebrewList::new(viewer.events, &mut viewer.ui.settings)
                .linking()
                .button_width(button_width)
                .rows(ui);
        });
}

#[cfg(not(target_arch = "wasm32"))]
fn recent_rows(viewer: &mut TabViewer, ui: &mut Ui, button_width: f32) -> bool {
    viewer.ui.recent.prune();
    viewer.ui.recent.name_from_files();
    if viewer.ui.recent.is_empty() {
        return false;
    }
    heading(ui, "Recent");
    let mut chosen = None;
    for entry in viewer.ui.recent.entries() {
        let folder = entry
            .path
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if row(ui, &entry.display_name(), &folder, button_width) {
            chosen = Some(entry.path.clone());
        }
    }
    if let Some(path) = chosen {
        match std::fs::read(&path) {
            Ok(data) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
                viewer.events.link_game(data, name);
            }
            Err(err) => {
                viewer
                    .events
                    .notify_error(format!("Could not open {}: {err}", path.display()));
                viewer.ui.recent.remove(&path);
            }
        }
    }
    ui.add_space(6.0);
    true
}

fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(RichText::new(text).strong());
    ui.add_space(2.0);
}

#[cfg(not(target_arch = "wasm32"))]
fn row(ui: &mut Ui, title: &str, detail: &str, link_width: f32) -> bool {
    let mut picked = false;
    ui.group(|ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let title_width = ui.available_width() - link_width - ui.spacing().item_spacing.x;
            ui.vertical(|ui| {
                ui.set_max_width(title_width);
                ui.style_mut().interaction.selectable_labels = false;
                ui.add(Label::new(RichText::new(title).strong().size(16.0)).truncate());
                ui.add(Label::new(RichText::new(detail).small()).truncate());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                picked = ui
                    .add(egui::Button::new("Link").min_size(egui::vec2(link_width, 0.0)))
                    .clicked();
            });
        });
    });
    picked
}

fn panel(viewer: &mut TabViewer, ui: &mut Ui) {
    if viewer.ui.settings.layout.is_touch(ui.ctx()) {
        panel_body(viewer, ui, false);
        return;
    }
    let output = egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| panel_body(viewer, ui, true));
    let overflow = output.content_size.y - output.inner_rect.height();
    let linked = &mut viewer.ui.linked;
    linked.fitted_height = Some((linked.screen_height - overflow).max(MIN_SCREEN_HEIGHT));
}

fn panel_body(viewer: &mut TabViewer, ui: &mut Ui, fit_height: bool) {
    let touch = viewer.ui.settings.layout.is_touch(ui.ctx());
    let Some(link) = viewer.emulator.link.as_ref() else {
        return;
    };
    let title = link.title().to_owned();
    egui::Sides::new().shrink_left().truncate().show(
        ui,
        |ui| {
            ui.label(RichText::new(console_name(link.model())).weak());
        },
        |ui| {
            ui.label(RichText::new(driver(link, Game::Second, touch)).weak());
        },
    );
    ui.add_space(6.0);

    let width = ui.available_width();
    let scale = (width / SCREEN_WIDTH as f32).min(MAX_SCREEN_SCALE);
    let mut height = SCREEN_HEIGHT as f32 * scale;
    if fit_height && let Some(fitted) = viewer.ui.linked.fitted_height {
        height = height.min(fitted);
    }
    viewer.ui.linked.screen_height = height;
    let clicks = link.takes_clicks() && link.control() != Game::Second;
    let sense = if clicks {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (area, response) = ui.allocate_exact_size(egui::vec2(width, height), sense);
    paint_screen(&mut viewer.ui.linked, ui, &link.gb, area, MAX_SCREEN_SCALE);
    if clicks
        && response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
    {
        viewer.emulator.set_link_control(Game::Second);
    }
    ui.add_space(6.0);

    let settings = &mut viewer.ui.settings;
    let Some(link) = viewer.emulator.link.as_mut() else {
        return;
    };
    let mut plugged = link.plugged();
    if ui.checkbox(&mut plugged, "Link cable plugged in").changed() {
        link.set_plugged(plugged);
    }

    let mut changed = false;
    let link_settings = &mut settings.local_link;
    ui.add_space(4.0);
    ui.label(RichText::new("Controls").strong());
    egui::ComboBox::from_id_salt("link_controls")
        .width(ui.available_width())
        .truncate()
        .selected_text(link_settings.controls.label())
        .show_ui(ui, |ui| {
            for controls in Controls::iter() {
                changed |= ui
                    .selectable_value(&mut link_settings.controls, controls, controls.label())
                    .changed();
            }
        });
    if link_settings.controls.sides().is_some() {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui
                .button(format!("{}  Swap inputs", icons::ARROWS_LEFT_RIGHT))
                .clicked()
            {
                link_settings.swapped = !link_settings.swapped;
                changed = true;
            }
            if link_settings.controls == Controls::SplitKeyboard
                && ui
                    .button(format!("{}  Change keys", icons::KEYBOARD))
                    .clicked()
            {
                settings_controls(viewer.events);
            }
        });
    }
    ui.add_space(4.0);
    changed |= ui
        .checkbox(
            &mut link_settings.second_sound,
            format!("Hear {title} too, quieter"),
        )
        .changed();
    if changed {
        link.set_settings(link_settings);
        settings.dirty = true;
    }

    ui.add_space(6.0);
    if wide_button(
        ui,
        egui::Button::new(format!("{}  Close {title}", icons::X)).truncate(),
    )
    .clicked()
    {
        viewer.events.end_link();
    }
}

fn settings_controls(events: &mut AppEventQueue) {
    events.open_settings(SettingsTab::Controls);
}

pub fn strip(ui: &mut Ui, link: &LocalLink) -> bool {
    strip_row(ui, icons::GAME_CONTROLLER, link.title(), link.plugged())
}

pub fn partner_strip(ui: &mut Ui, partner: &crate::partner_link::PartnerLink) -> bool {
    strip_row(
        ui,
        icons::HANDSHAKE,
        &format!("Event partner: {}", partner.name()),
        partner.plugged(),
    )
}

fn strip_row(ui: &mut Ui, icon: &str, title: &str, plugged: bool) -> bool {
    let height = ui.spacing().interact_size.y;
    let response = ui
        .allocate_ui_with_layout(
            egui::vec2(ui.available_width(), height),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.label(RichText::new(icon).weak());
                ui.add(Label::new(title).truncate());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(icons::CARET_DOWN).weak());
                    let (icon, word) = if plugged {
                        (icons::PLUGS_CONNECTED, "Plugged in")
                    } else {
                        (icons::PLUGS, "Unplugged")
                    };
                    ui.label(format!("{icon} {word}"));
                });
            },
        )
        .response;
    ui.interact(
        response.rect,
        egui::Id::new("link_strip"),
        egui::Sense::click(),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
    .clicked()
}

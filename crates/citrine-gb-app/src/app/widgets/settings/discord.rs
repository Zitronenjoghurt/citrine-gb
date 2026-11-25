use crate::app::ui_state::settings::Settings;
use crate::app::ui_state::settings_tree::Surface;
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::widgets::generic_select::GenericSelect;
use crate::app::widgets::settings::form::Form;
use crate::app::widgets::settings::pages::app::{
    DISCORD_DETAILS_HINT, DISCORD_PICTURES_OFF, DISCORD_SCREENSHOT_HINT,
};
use crate::discord::{GameNow, Presence, ShotInterval, ShotScreens, presence};
use egui::{CornerRadius, RichText, Ui, Widget, vec2};

pub fn row(form: &mut Form, s: &mut Settings, row: RowId, pictures_off: bool) {
    let label = row.label(Surface::Desktop);
    let d = &mut s.discord;
    let mut dirty = false;
    match row {
        RowId::DiscordActivity => dirty |= form.toggle(label, &mut d.enabled),
        RowId::DiscordGame if d.enabled => dirty |= form.toggle(label, &mut d.game),
        RowId::DiscordDetails if d.enabled => {
            dirty |= form.toggle_with_hint(label, DISCORD_DETAILS_HINT, &mut d.details);
        }
        RowId::DiscordTime if d.enabled => dirty |= form.toggle(label, &mut d.play_time),
        RowId::DiscordShot if d.enabled => {
            let mut off = false;
            let shot = if pictures_off {
                &mut off
            } else {
                &mut d.screenshot
            };
            dirty |= form.toggle_when(label, Some(DISCORD_SCREENSHOT_HINT), !pictures_off, shot);
            if pictures_off {
                form.hint(DISCORD_PICTURES_OFF);
            }
        }
        RowId::DiscordEvery if d.enabled && d.screenshot && !pictures_off => {
            form.row(label, |ui| {
                dirty |= GenericSelect::from_enum(&mut d.every, "enum_select_discord_every")
                    .default_value(ShotInterval::default())
                    .ui(ui)
                    .changed();
            });
        }
        RowId::DiscordDsScreens if d.enabled && d.screenshot && !pictures_off => {
            form.row(label, |ui| {
                dirty |= GenericSelect::from_enum(&mut d.screens, "enum_select_discord_screens")
                    .default_value(ShotScreens::default())
                    .ui(ui)
                    .changed();
            });
        }
        _ => {}
    }
    s.dirty |= dirty;
}

pub fn preview(
    form: &mut Form,
    s: &Settings,
    game: Option<&GameNow>,
    screen: Option<egui::TextureId>,
    pictures_off: bool,
) {
    if let Some(shown) = presence(&s.discord, game, None) {
        form.custom(|ui| {
            ui.weak("Your Discord profile");
            card(ui, &shown, screen.filter(|_| !pictures_off));
        });
    }
}

fn aspect_of(ui: &Ui, texture: egui::TextureId) -> f32 {
    let size = ui
        .ctx()
        .tex_manager()
        .read()
        .meta(texture)
        .map(|meta| meta.size);
    let [width, height] = size.unwrap_or_else(|| {
        let video = citrine_gb::console::ConsoleKind::Gb.video();
        [video.width, video.height]
    });
    height as f32 / width as f32
}

fn card(ui: &mut Ui, shown: &Presence, screen: Option<egui::TextureId>) {
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let size = vec2(64.0, 64.0);
                let image = match screen.filter(|_| shown.screenshot) {
                    Some(texture) => ui.add(
                        egui::Image::new((texture, vec2(64.0 / aspect_of(ui, texture), 64.0)))
                            .corner_radius(CornerRadius::same(6)),
                    ),
                    None => match crate::app::widgets::app_icon::texture(ui.ctx()) {
                        Some(icon) => ui.add(
                            egui::Image::new((icon.id(), size))
                                .corner_radius(CornerRadius::same(6)),
                        ),
                        None => ui.allocate_exact_size(size, egui::Sense::hover()).1,
                    },
                };
                if let Some(hover) = &shown.hover {
                    image.on_hover_text(hover);
                }
                ui.vertical(|ui| {
                    ui.label(RichText::new("Citrine").strong());
                    ui.add(egui::Label::new(&shown.details).wrap());
                    if let Some(state) = &shown.state {
                        ui.add(egui::Label::new(state).truncate());
                    }
                    if let Some(started) = shown.started_at {
                        let secs = crate::storage::now_unix().saturating_sub(started);
                        ui.weak(format!(
                            "{:02}:{:02}:{:02} elapsed",
                            secs / 3600,
                            secs / 60 % 60,
                            secs % 60
                        ));
                    }
                });
            });
        });
}

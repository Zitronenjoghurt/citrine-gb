use crate::app::ui_state::settings::Settings;
use crate::app::widgets::generic_select::GenericSelect;
use crate::app::widgets::section::section_with_hint;
use crate::app::widgets::settings::form::Form;
use crate::discord::{GameNow, Presence, ShotInterval, presence};
use crate::icons;
use egui::{CornerRadius, RichText, Ui, Widget, vec2};

const DISCORD_NOTE: &str =
    "Your Discord profile shows what you play while Discord runs on this computer.";

pub fn section(
    ui: &mut Ui,
    s: &mut Settings,
    game: Option<&GameNow>,
    screen: Option<egui::TextureId>,
    pictures_off: bool,
) {
    section_with_hint(ui, icons::DISCORD_LOGO, "Discord", DISCORD_NOTE, |ui| {
        Form::show(ui, "discord", |form| {
            form.row("Show activity", |ui| {
                s.dirty |= ui
                    .add(egui::Checkbox::without_text(&mut s.discord.enabled))
                    .changed();
            });
            if !s.discord.enabled {
                return;
            }
            form.row("Game name", |ui| {
                s.dirty |= ui
                    .add(egui::Checkbox::without_text(&mut s.discord.game))
                    .changed();
            });
            form.row_with_hint(
                "Details",
                "In Pokémon games: badges, Pokédex and play time. Hovering over the image shows your first Pokémon and money.",
                |ui| {
                    s.dirty |= ui
                        .add(egui::Checkbox::without_text(&mut s.discord.details))
                        .changed();
                },
            );
            form.row("Session time", |ui| {
                s.dirty |= ui
                    .add(egui::Checkbox::without_text(&mut s.discord.play_time))
                    .changed();
            });
            form.row_with_hint(
                "Screenshot",
                "Your Discord profile shows your game screen instead of the Citrine icon.",
                |ui| {
                    if pictures_off {
                        ui.add_enabled(false, egui::Checkbox::without_text(&mut false));
                    } else {
                        s.dirty |= ui
                            .add(egui::Checkbox::without_text(&mut s.discord.screenshot))
                            .changed();
                    }
                },
            );
            if pictures_off {
                form.hint("Off on this server.");
            } else if s.discord.screenshot {
                form.row("Every", |ui| {
                    s.dirty |=
                        GenericSelect::from_enum(&mut s.discord.every, "enum_select_discord_every")
                            .default_value(ShotInterval::default())
                            .ui(ui)
                            .changed();
                });
            }
        });
        if let Some(shown) = presence(&s.discord, game, None) {
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.add_space(6.0);
                ui.weak("Your Discord profile");
                card(ui, &shown, screen.filter(|_| !pictures_off));
            });
        }
    });
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
                        egui::Image::new((texture, vec2(64.0 * 160.0 / 144.0, 64.0)))
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
                    ui.add(egui::Label::new(&shown.details).truncate());
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

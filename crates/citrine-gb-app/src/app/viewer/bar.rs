use super::{Act, BAR, Facts};
use crate::app::tabs::TabViewer;
use crate::app::widgets::action::{action_height, icon_button, icon_text};
use crate::app::widgets::emoji;
use crate::icons;
use crate::media::playback::{self};
use egui::{Align, Layout, Rect, RichText, Ui, vec2};

pub(super) fn top_bar(ui: &mut Ui, rect: Rect, facts: &Facts, mobile: bool, acts: &mut Vec<Act>) {
    ui.painter()
        .rect_filled(rect, 0.0, ui.visuals().panel_fill.gamma_multiply(0.92));
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(8.0, 4.0)))
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            let side = (BAR - 12.0).min(action_height(ui));
            if ui
                .add(icon_button(icons::X, side))
                .on_hover_text("Close")
                .clicked()
            {
                acts.push(Act::Close);
            }
            ui.add_space(4.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let button = |ui: &mut Ui, icon: &str, label: &str| {
                    let text = if mobile {
                        icon_text(icon)
                    } else {
                        RichText::new(format!("{icon} {label}"))
                    };
                    ui.add(egui::Button::new(text).min_size(vec2(side, side)))
                        .on_hover_text(label)
                        .clicked()
                };
                if facts.deletable && button(ui, icons::TRASH, "Delete") {
                    acts.push(Act::Delete);
                }
                #[cfg(not(target_arch = "wasm32"))]
                if facts.deletable && button(ui, icons::FOLDER_OPEN, crate::media::reveal::LABEL) {
                    acts.push(Act::Reveal);
                }
                if facts.can_send && button(ui, icons::PAPER_PLANE_TILT, "Send") {
                    acts.push(Act::Send);
                }
                if facts.deletable && button(ui, icons::DOWNLOAD_SIMPLE, "Export") {
                    acts.push(Act::Export);
                }
                if let Some(way) = facts.pass
                    && button(ui, way.icon(), way.label())
                {
                    acts.push(Act::Pass(way));
                }
                if facts.can_save {
                    if facts.saved {
                        let text = if mobile {
                            icon_text(icons::CHECK)
                        } else {
                            RichText::new(format!("{} Saved to Media", icons::CHECK))
                        };
                        ui.add_enabled(false, egui::Button::new(text).min_size(vec2(side, side)))
                            .on_disabled_hover_text("Saved to Media");
                    } else if button(ui, icons::DOWNLOAD_SIMPLE, "Save to Media") {
                        acts.push(Act::Save);
                    }
                }
                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    let block = ui.text_style_height(&egui::TextStyle::Body)
                        + 1.0
                        + ui.text_style_height(&egui::TextStyle::Small);
                    ui.add_space(((ui.available_height() - block) / 2.0).max(0.0));
                    let strong = ui.visuals().strong_text_color();
                    let title = format!("{} {}", facts.icon, facts.title);
                    emoji::line(ui, &title, egui::TextStyle::Body, strong);
                    let line = if mobile { &facts.short } else { &facts.line };
                    let color = ui.visuals().text_color();
                    emoji::line(ui, line, egui::TextStyle::Small, color);
                });
            });
        },
    );
}

pub(super) fn controls(
    ui: &mut Ui,
    rect: Rect,
    viewer: &mut TabViewer,
    mobile: bool,
    acts: &mut Vec<Act>,
) {
    ui.painter()
        .rect_filled(rect, 0.0, ui.visuals().panel_fill.gamma_multiply(0.92));
    let state = &viewer.ui.viewer;
    let player = state.shown.as_ref().and_then(|s| s.player.as_ref());
    let enabled = player.is_some_and(|p| p.is_ready());
    let playing = player.is_some_and(|p| p.is_playing());
    let position = player.map_or(0.0, |p| p.position());
    let length = player.map_or(0.0, |p| p.length());
    let muted = state.muted;
    let volume = state.volume;
    let silent = player.is_some_and(|p| p.is_silent_here());
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(10.0, 6.0)))
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                let side = action_height(ui);
                let (icon, hint) = if playing {
                    (icons::PAUSE, "Pause")
                } else {
                    (icons::PLAY, "Play")
                };
                if ui
                    .add(icon_button(icon, side))
                    .on_hover_text(hint)
                    .clicked()
                {
                    acts.push(Act::Toggle);
                }
                ui.label(RichText::new(playback::clock(position)).monospace().small());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if !mobile && !silent {
                        let mut level = volume;
                        let slider = ui.add(
                            egui::Slider::new(&mut level, 0.0..=1.0)
                                .show_value(false)
                                .trailing_fill(true),
                        );
                        if slider.changed() {
                            acts.push(Act::Volume(level));
                        }
                    }
                    let (icon, hint) = if silent {
                        (
                            icons::SPEAKER_SLASH,
                            "This recording's sound can't play here",
                        )
                    } else if muted || volume == 0.0 {
                        (icons::SPEAKER_SLASH, "Sound on")
                    } else {
                        (icons::SPEAKER_HIGH, "Mute")
                    };
                    let mute = ui
                        .add_enabled(!silent, icon_button(icon, side))
                        .on_hover_text(hint)
                        .on_disabled_hover_text(hint);
                    if mute.clicked() {
                        acts.push(Act::Mute);
                    }
                    ui.label(RichText::new(playback::clock(length)).monospace().small());
                    let mut at = position as f32;
                    ui.spacing_mut().slider_width = ui.available_width() - 8.0;
                    let seek = ui.add(
                        egui::Slider::new(&mut at, 0.0..=(length.max(0.01) as f32))
                            .show_value(false)
                            .trailing_fill(true),
                    );
                    if seek.changed() {
                        acts.push(Act::Seek(at as f64));
                    }
                });
            });
        },
    );
}

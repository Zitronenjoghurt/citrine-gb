use super::share::replay_share;
use super::{COMPOSER_ROWS, FIELD_MARGIN, PICKER_EMOJI, PICKER_HEIGHT};
use crate::app::events::AppEvent;
use crate::app::tabs::TabViewer;
use crate::app::widgets::emoji;
use crate::icons;
use crate::media::{self};
use citrine_gb_net::{MAX_TEXT_CHARS, OnlineId};
use egui::{Align, Key, Layout, Modifiers, RichText, Ui, vec2};

pub(super) fn composer(ui: &mut Ui, viewer: &mut TabViewer, friend: OnlineId) {
    let row = ui.text_style_height(&egui::TextStyle::Body);
    let side = ui.spacing().interact_size.y.max(row + FIELD_MARGIN.sum().y);
    let gap = ui.spacing().item_spacing.x / 2.0;
    let id = super::draft_id(friend);
    let height_id = egui::Id::new(("chat_field_height", friend));
    let tall = ui
        .data(|d| d.get_temp::<f32>(height_id))
        .unwrap_or(side)
        .max(side);
    let mut send = false;
    let editing = viewer.ui.friends.chat.editing(friend);
    if editing.is_some() && edit_bar(ui) {
        viewer.ui.friends.chat.end_edit();
        return;
    }
    if editing.is_some()
        && ui.memory(|m| m.has_focus(id))
        && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape))
    {
        viewer.ui.friends.chat.end_edit();
        return;
    }
    let focused = ui.memory(|m| m.has_focus(id));
    let leave = std::mem::take(&mut viewer.ui.friends.chat.leave_draft);
    if leave
        || editing.is_none()
            && focused
            && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape))
    {
        ui.memory_mut(|m| m.surrender_focus(id));
    }
    if std::mem::take(&mut viewer.ui.friends.chat.focus_draft) {
        ui.memory_mut(|m| m.request_focus(id));
    }
    let to_game = !viewer.ui.friends.chat.touch && viewer.emulator.open_game().is_some();
    let mut enter = false;
    ui.add_space(2.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        at_foot(ui, side, tall, |ui| attach_menu(ui, viewer, friend, side));
        let state = &mut viewer.ui.friends.chat;
        enter = ui.memory(|m| m.has_focus(id)) && ui.input_mut(enter_sends);
        send |= enter;
        let draft = state.drafts.entry(friend).or_default();
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let ready = !draft.trim().is_empty();
            let (icon, tip) = match editing {
                Some(_) => (icons::CHECK, "Save"),
                None => (icons::PAPER_PLANE_RIGHT, "Send"),
            };
            let send_button = at_foot(ui, side, tall, |ui| {
                ui.add_enabled_ui(ready, |ui| icon_button(ui, egui::Button::new(icon), side))
                    .inner
            })
            .on_hover_text(tip);
            send |= send_button.clicked();
            let picker = state.picker;
            let label = if picker { "Hide emoji" } else { "Emoji" };
            if at_foot(ui, side, tall, |ui| {
                icon_button(ui, egui::Button::new(icons::SMILEY).selected(picker), side)
            })
            .on_hover_text(label)
            .clicked()
            {
                state.picker = !picker;
            }
            let bar = ui.spacing().scroll.allocated_width();
            let width = (ui.available_width() - bar).max(side);
            let (field, shown) = draft_field(ui, draft, id, width, side, "Message");
            if (shown - tall).abs() >= 0.5 {
                ui.data_mut(|d| d.insert_temp(height_id, shown));
                ui.ctx().request_discard("chat field height");
            }
            if field.changed() && ready == draft.trim().is_empty() {
                ui.ctx().request_repaint();
            }
        });
    });
    if !send {
        return;
    }
    let state = &mut viewer.ui.friends.chat;
    let draft = state.drafts.entry(friend).or_default();
    match editing {
        Some(message) => {
            if viewer.online.edit_text(friend, message, draft) {
                state.end_edit();
            }
        }
        None => {
            if viewer.online.send_text(friend, draft) {
                draft.clear();
                state.jump = true;
            }
        }
    }
    if to_game && enter {
        ui.memory_mut(|m| m.surrender_focus(id));
    } else {
        ui.memory_mut(|m| m.request_focus(id));
    }
}

fn edit_bar(ui: &mut Ui) -> bool {
    let mut cancel = false;
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("{} Editing message", icons::PENCIL_SIMPLE))
                .small()
                .color(ui.visuals().weak_text_color()),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            cancel = ui
                .small_button(icons::X)
                .on_hover_text("Cancel editing")
                .clicked();
        });
    });
    cancel
}

pub(super) fn draft_field(
    ui: &mut Ui,
    draft: &mut String,
    id: egui::Id,
    width: f32,
    side: f32,
    hint: &str,
) -> (egui::Response, f32) {
    let row = ui.text_style_height(&egui::TextStyle::Body);
    let mut layouter = |ui: &Ui, text: &dyn egui::TextBuffer, wrap: f32| {
        emoji::field_galley(ui, text.as_str(), wrap)
    };
    let scroll = egui::ScrollArea::vertical()
        .id_salt(id.with("scroll"))
        .max_height(row * COMPOSER_ROWS + 8.0)
        .stick_to_bottom(true)
        .show(ui, |ui| {
            let output = egui::TextEdit::multiline(draft)
                .id(id)
                .hint_text(hint)
                .desired_rows(1)
                .char_limit(MAX_TEXT_CHARS)
                .margin(FIELD_MARGIN)
                .desired_width(width - FIELD_MARGIN.sum().x)
                .min_size(vec2(width, side))
                .vertical_align(Align::Center)
                .layouter(&mut layouter)
                .show(ui);
            emoji::paint_field(ui, &output, draft);
            output.response
        });
    let shown = scroll.inner.rect.height().min(scroll.inner_rect.height());
    (scroll.inner, shown)
}

pub(super) fn enter_sends(input: &mut egui::InputState) -> bool {
    !input.modifiers.shift && input.consume_key(Modifiers::NONE, Key::Enter)
}

pub(super) fn at_foot<R>(ui: &mut Ui, side: f32, tall: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.allocate_ui_with_layout(vec2(side, tall), Layout::bottom_up(Align::Center), add)
        .inner
}

pub(super) fn icon_button(ui: &mut Ui, button: egui::Button, side: f32) -> egui::Response {
    ui.add_sized(vec2(side, side), button)
}

pub(super) fn attach_menu(ui: &mut Ui, viewer: &mut TabViewer, friend: OnlineId, side: f32) {
    let button = icon_button(ui, egui::Button::new(icons::PLUS), side).on_hover_text("Share");
    let has_rom = viewer.emulator.has_game();
    let has_media = !viewer.media.items().is_empty();
    let has_replay = viewer.emulator.recorder.has_data();
    let blocked = viewer.online.share_blocked();
    egui::Popup::menu(&button).show(|ui| {
        if ui
            .add_enabled(
                has_rom && blocked.is_none(),
                egui::Button::new(format!("{}  Screenshot now", icons::CAMERA)),
            )
            .on_disabled_hover_text(blocked.unwrap_or("Load a ROM first"))
            .clicked()
        {
            let scale = viewer.ui.settings.screenshot_scale;
            match media::save_screenshot(viewer.emulator, viewer.media, scale) {
                Ok(item) => {
                    viewer
                        .ui
                        .friends
                        .chat
                        .send_media(viewer.media, friend, &item);
                    viewer.events.push(AppEvent::MediaAdded { id: item.id });
                }
                Err(err) => viewer.events.notify_error(err),
            }
        }
        if ui
            .add_enabled(
                has_media && blocked.is_none(),
                egui::Button::new(format!("{}  From Media", icons::IMAGES)),
            )
            .on_disabled_hover_text(blocked.unwrap_or("Media has no screenshots or recordings yet"))
            .clicked()
        {
            viewer.ui.friends.chat.from_media = true;
        }
        let palettes = viewer.ui.settings.palettes.entries();
        if palettes.is_empty() {
            ui.add_enabled(
                false,
                egui::Button::new(format!("{}  Palette", icons::PALETTE)),
            )
            .on_disabled_hover_text("Make a palette in Settings first");
        } else {
            let mut picked = None;
            ui.menu_button(format!("{}  Palette", icons::PALETTE), |ui| {
                for palette in palettes {
                    if ui.button(&palette.name).clicked() {
                        picked = Some(palette.to_shared());
                    }
                }
            });
            if let Some(shared) = picked {
                if viewer.online.send_palette(friend, shared) {
                    viewer.ui.friends.chat.jump = true;
                } else {
                    viewer.events.notify_error("Couldn't share the palette");
                }
            }
        }
        if has_replay
            && ui
                .add_enabled(
                    blocked.is_none(),
                    egui::Button::new(format!("{}  Input replay", icons::GAME_CONTROLLER)),
                )
                .on_disabled_hover_text(blocked.unwrap_or_default())
                .clicked()
        {
            let shared =
                replay_share(viewer.emulator).is_some_and(|new| viewer.online.share(friend, new));
            if shared {
                viewer.ui.friends.chat.jump = true;
            } else {
                viewer.events.notify_error("Couldn't share the replay");
            }
        }
    });
}

pub(super) fn picker(ui: &mut Ui, viewer: &mut TabViewer, friend: OnlineId) {
    ui.add_space(4.0);
    let mut picked = None;
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(8.0)
        .inner_margin(6.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical()
                .id_salt("chat_emoji_scroll")
                .max_height(PICKER_HEIGHT)
                .show(ui, |ui| picked = emoji::picker(ui, PICKER_EMOJI));
        });
    if let Some(glyph) = picked {
        let state = &mut viewer.ui.friends.chat;
        let draft = state.drafts.entry(friend).or_default();
        if draft.chars().count() + glyph.chars().count() <= MAX_TEXT_CHARS {
            draft.push_str(glyph);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::texts;
    use super::super::*;
    use super::{draft_field, enter_sends, icon_button};

    #[test]
    fn an_icon_button_in_the_message_row_has_its_icon_in_the_middle() {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(412.0, 200.0),
            )),
            ..Default::default()
        };
        let mut button = None;
        let output = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    button = Some(icon_button(ui, egui::Button::new(icons::SMILEY), 44.0).rect);
                });
            });
        });
        let button = button.expect("the button");
        let icon = texts(&output.shapes)
            .into_iter()
            .find(|(text, _)| text == icons::SMILEY)
            .expect("the icon")
            .1;
        assert!(
            (icon.center().y - button.center().y).abs() < 2.0,
            "{icon:?} {button:?}"
        );
    }

    #[test]
    fn enter_sends_and_shift_enter_starts_a_new_line() {
        for (modifiers, sends) in [(Modifiers::NONE, true), (Modifiers::SHIFT, false)] {
            let ctx = egui::Context::default();
            let input = egui::RawInput {
                modifiers,
                events: vec![egui::Event::Key {
                    key: Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                }],
                ..Default::default()
            };
            let mut sent = None;
            let _ = ctx.run(input, |ctx| sent = Some(ctx.input_mut(enter_sends)));
            assert_eq!(sent, Some(sends), "{modifiers:?}");
        }
    }

    #[test]
    fn the_empty_field_is_one_row_tall_and_grows_with_lines() {
        for (draft, grows) in [("", false), ("one\ntwo\nthree", true)] {
            let ctx = egui::Context::default();
            ctx.set_pixels_per_point(2.0);
            let mut draft = draft.to_owned();
            let mut side = 0.0;
            let mut shown = 0.0;
            for _ in 0..2 {
                let _ = ctx.run(egui::RawInput::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let row = ui.text_style_height(&egui::TextStyle::Body);
                        side = ui.spacing().interact_size.y.max(row + FIELD_MARGIN.sum().y);
                        let id = egui::Id::new("draft");
                        shown = draft_field(ui, &mut draft, id, 200.0, side, "Message").1;
                    });
                });
            }
            if grows {
                assert!(shown > side + 10.0, "{shown} grows past {side}");
            } else {
                assert!((shown - side).abs() < 1.0, "{shown} is one row of {side}");
            }
        }
    }
}

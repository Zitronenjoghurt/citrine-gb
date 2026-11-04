use crate::app::tabs::TabViewer;
use crate::app::ui_state::ui_theme::ink_on;
use crate::app::widgets::avatar;
use crate::icons;
use crate::online::Online;
use crate::play::Play;
use citrine_gb_net::{Body, OnlineId};
use egui::{Align, Label, Layout, RichText, TextEdit, Ui, vec2};

const FACE: f32 = 22.0;
const TOUCH_FACE: f32 = 32.0;
const TOUCH_TEXT: f32 = 16.0;
const TOUCH_NAME: f32 = 14.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatLine {
    pub from: OnlineId,
    pub mine: bool,
    pub name: String,
    pub text: String,
}

pub fn lines(play: &Play, online: &Online) -> Vec<ChatLine> {
    let Some(room) = play.room() else {
        return Vec::new();
    };
    let me = online.account().map(|a| a.online_id);
    room.chat()
        .iter()
        .filter_map(|line| {
            let Body::Text(text) = &line.envelope.body else {
                return None;
            };
            let name = room
                .member(line.from)
                .map_or_else(|| online.display_name(line.from), |m| m.name.clone());
            Some(ChatLine {
                from: line.from,
                mine: Some(line.from) == me,
                name,
                text: text.clone(),
            })
        })
        .collect()
}

fn text(line: &ChatLine, touch: bool) -> RichText {
    let text = RichText::new(&line.text);
    if touch { text.size(TOUCH_TEXT) } else { text }
}

fn theirs(ui: &mut Ui, line: &ChatLine, first: bool, touch: bool) {
    let face = if touch { TOUCH_FACE } else { FACE };
    ui.horizontal_top(|ui| {
        if first {
            avatar::avatar(ui, &line.name, line.from.value(), face, None);
        } else {
            ui.add_space(face);
        }
        ui.vertical(|ui| {
            if first {
                let name = RichText::new(crate::app::tabs::playing::short_name(&line.name));
                let name = if touch {
                    name.size(TOUCH_NAME).color(ui.visuals().text_color())
                } else {
                    name.small().weak()
                };
                ui.label(name);
            }
            ui.add(Label::new(text(line, touch)).wrap());
        });
    });
}

fn mine(ui: &mut Ui, line: &ChatLine, touch: bool) {
    let fill = ui.visuals().selection.bg_fill;
    let most = ui.available_width() * 0.8;
    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
        egui::Frame::new()
            .fill(fill)
            .corner_radius(12)
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_max_width(most);
                ui.add(Label::new(text(line, touch).color(ink_on(fill))).wrap());
            });
    });
}

pub fn body(ui: &mut Ui, lines: &[ChatLine], draft: &mut String, touch: bool) -> Option<String> {
    let mut sent = None;
    let side = if touch {
        ui.spacing().interact_size.y.max(40.0)
    } else {
        ui.spacing().interact_size.y
    };
    ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
        ui.horizontal(|ui| {
            let send = egui::Button::new(icons::PAPER_PLANE_TILT).min_size(vec2(side, side));
            let room = ui.available_width() - side - ui.spacing().item_spacing.x;
            let field = ui.add(
                TextEdit::singleline(draft)
                    .hint_text("Message the room")
                    .desired_width(room)
                    .min_size(vec2(room, side))
                    .vertical_align(Align::Center)
                    .id_salt("room_chat_draft"),
            );
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let clicked = ui
                .add_enabled(!draft.trim().is_empty(), send)
                .on_hover_text("Send")
                .clicked();
            if (entered || clicked) && !draft.trim().is_empty() {
                sent = Some(draft.trim().to_owned());
                if entered {
                    field.request_focus();
                }
            }
        });
        ui.add_space(6.0);
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("room_chat_lines")
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    let mut last = None;
                    for line in lines {
                        let first = last != Some(line.from);
                        if first && last.is_some() {
                            ui.add_space(4.0);
                        }
                        if line.mine {
                            mine(ui, line, touch);
                        } else {
                            theirs(ui, line, first, touch);
                        }
                        last = Some(line.from);
                    }
                });
        });
    });
    sent
}

pub fn send(play: &mut Play, online: &mut Online, draft: &mut String, text: String) -> bool {
    if !play.say(online, text) {
        return false;
    }
    draft.clear();
    true
}

pub fn show(viewer: &mut TabViewer, ui: &mut Ui) {
    let lines = lines(viewer.play, viewer.online);
    viewer.ui.playing.room_seen = viewer.play.room().map_or(0, |r| r.chat().len());
    let draft = &mut viewer.ui.playing.room_draft;
    if let Some(text) = body(ui, &lines, draft, false)
        && !send(viewer.play, viewer.online, draft, text)
    {
        viewer.events.notify_error("Couldn't send the message");
    }
}

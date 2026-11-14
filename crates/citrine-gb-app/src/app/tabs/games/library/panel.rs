use super::count_words;
use super::info::{CREDIT, GameCard, tag_row_label, tag_rows};
use crate::app::big_screen::Cover;
use crate::app::events::GamesAction;
use crate::app::ui_state::library::Library;
use crate::app::widgets::action::{dismiss_button, main_button, quiet_button, wide_button};
use crate::app::widgets::tag_chip::{Look, TagChip, Tap};
use crate::icons;
use egui::{Align, Id, Key, Layout, Rect, RichText, ScrollArea, Ui, vec2};

pub const PANEL_WIDTH: f32 = 300.0;
const EDGE: f32 = 12.0;
const COVER_HEIGHT: f32 = 240.0;
const GROUP_GAP: f32 = 12.0;
const LABEL_GAP: f32 = 4.0;
const FIELD_WIDTH: f32 = 120.0;
const SUGGEST_WIDTH: f32 = 220.0;

pub fn panel(
    ui: &mut Ui,
    card: &GameCard,
    library: &Library,
    draft: &mut Option<String>,
) -> Option<GamesAction> {
    let mut action = None;
    ScrollArea::vertical()
        .id_salt("library_info")
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.set_max_width(ui.available_width() - EDGE);
            ui.add_space(EDGE);
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    if ui.add(dismiss_button()).on_hover_text("Close").clicked() {
                        action = Some(GamesAction::CloseInfo);
                    }
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        ui.add(
                            egui::Label::new(RichText::new(&card.name).size(16.0).strong()).wrap(),
                        );
                        if let Some(console) = card.console {
                            ui.label(RichText::new(console).weak().small());
                        }
                    });
                });
            });
            ui.add_space(GROUP_GAP);
            cover(ui, card.cover.as_ref());
            ui.add_space(GROUP_GAP);
            if card.playable
                && wide_button(ui, main_button(ui, format!("{} Play", icons::PLAY))).clicked()
            {
                action = Some(GamesAction::Play(card.path.clone()));
            }
            label(ui, "Tags");
            if let Some(picked) = tags(ui, card, library, draft) {
                action = Some(picked);
            }
            if !card.genres.is_empty() {
                label(ui, "Genres");
                ui.horizontal_wrapped(|ui| {
                    for genre in &card.genres {
                        TagChip::new(genre, Look::Genre).show(ui);
                    }
                });
            }
            label(ui, "About");
            let about = card.about_rows();
            if !about.is_empty() {
                egui::Grid::new("library_info_about")
                    .num_columns(2)
                    .spacing(vec2(16.0, LABEL_GAP))
                    .show(ui, |ui| {
                        for (name, value) in about {
                            ui.label(RichText::new(name).weak());
                            ui.label(value);
                            ui.end_row();
                        }
                    });
                ui.add_space(LABEL_GAP);
            }
            for (name, url) in &card.links {
                ui.hyperlink_to(format!("{name} {}", icons::ARROW_SQUARE_OUT), url);
            }
            if card.has_facts() {
                ui.add_space(LABEL_GAP);
                ui.label(RichText::new(CREDIT).weak().small());
            }
            ui.add_space(GROUP_GAP * 2.0);
            let remove = quiet_button(ui, format!("{} Remove from library", icons::TRASH));
            if wide_button(ui, remove).clicked() {
                action = Some(GamesAction::RemoveGame(card.path.clone()));
            }
            ui.add_space(EDGE);
        });
    action
}

fn label(ui: &mut Ui, text: &str) {
    ui.add_space(GROUP_GAP);
    ui.label(RichText::new(text).weak().small());
    ui.add_space(LABEL_GAP);
}

fn cover(ui: &mut Ui, cover: Option<&Cover>) {
    let Some(cover) = cover else {
        return;
    };
    let room = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), COVER_HEIGHT));
    let scale = (room.width() / cover.size.x).min(room.height() / cover.size.y);
    let size = cover.size * scale;
    let (rect, _) = ui.allocate_exact_size(vec2(room.width(), size.y), egui::Sense::hover());
    let at = Rect::from_center_size(rect.center(), size);
    egui::Image::new((cover.id(), cover.size))
        .uv(cover.uv)
        .corner_radius(ui.visuals().widgets.noninteractive.corner_radius)
        .paint_at(ui, at);
}

fn tags(
    ui: &mut Ui,
    card: &GameCard,
    library: &Library,
    draft: &mut Option<String>,
) -> Option<GamesAction> {
    let mut action = None;
    ui.horizontal_wrapped(|ui| {
        for tag in &card.tags {
            if TagChip::new(tag, Look::Own).removable().show(ui) == Some(Tap::Remove) {
                action = Some(GamesAction::RemoveTag(card.path.clone(), tag.clone()));
            }
        }
        match draft {
            Some(typed) => {
                let (picked, close) = tag_field(ui, library, &card.path, typed);
                action = action.take().or(picked);
                if close {
                    *draft = None;
                }
            }
            None => {
                if TagChip::new("Add tag", Look::Add).show(ui).is_some() {
                    *draft = Some(String::new());
                }
            }
        }
    });
    action
}

fn tag_field(
    ui: &mut Ui,
    library: &Library,
    path: &str,
    typed: &mut String,
) -> (Option<GamesAction>, bool) {
    let field_id = Id::new("library_tag_field");
    let response = ui.add(
        egui::TextEdit::singleline(typed)
            .id(field_id)
            .hint_text("Tag name")
            .desired_width(FIELD_WIDTH),
    );
    if typed.is_empty() && !response.has_focus() && !response.lost_focus() {
        response.request_focus();
    }
    if ui.input(|input| input.key_pressed(Key::Escape)) {
        return (None, true);
    }
    if response.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
        let tag = typed.trim().to_owned();
        let picked = (!tag.is_empty()).then(|| GamesAction::AddTag(path.to_owned(), tag));
        return (picked, true);
    }
    let rows = tag_rows(library, path, typed);
    let mut picked = None;
    let mut menu = Rect::NOTHING;
    if !rows.is_empty() {
        let shown = egui::Area::new(Id::new("library_tag_suggest"))
            .order(egui::Order::Foreground)
            .fixed_pos(response.rect.left_bottom() + vec2(0.0, LABEL_GAP))
            .show(ui.ctx(), |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    ui.set_width(SUGGEST_WIDTH);
                    for row in &rows {
                        let icon = if row.new { icons::PLUS } else { icons::TAG };
                        let mut button =
                            egui::Button::new(format!("{icon} {}", tag_row_label(row)))
                                .frame(false)
                                .min_size(vec2(SUGGEST_WIDTH, 0.0));
                        if !row.new {
                            button = button.shortcut_text(count_words(row.count));
                        }
                        if ui.add(button).clicked() {
                            picked = Some(GamesAction::AddTag(path.to_owned(), row.name.clone()));
                        }
                    }
                });
            });
        menu = shown.response.rect;
    }
    if picked.is_some() {
        return (picked, true);
    }
    let outside = ui.input(|input| {
        input.pointer.any_click()
            && input
                .pointer
                .interact_pos()
                .is_some_and(|at| !response.rect.contains(at) && !menu.contains(at))
    });
    (None, outside)
}

use crate::app::events::AppEventQueue;
use crate::homebrew::HOMEBREW_GAMES;
use crate::icons;
use egui::{Response, ScrollArea, Ui, Widget};

pub struct HomebrewList<'a> {
    events: &'a mut AppEventQueue,
}

impl<'a> HomebrewList<'a> {
    pub fn new(events: &'a mut AppEventQueue) -> Self {
        Self { events }
    }
}

impl Widget for HomebrewList<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        ScrollArea::vertical()
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    for game in HOMEBREW_GAMES {
                        ui.group(|ui| {
                            ui.set_width(ui.available_width());

                            let id = details_id(game.id);
                            let mut state =
                                egui::collapsing_header::CollapsingState::load_with_default_open(
                                    ui.ctx(),
                                    id,
                                    false,
                                );
                            let header = ui.horizontal(|ui| {
                                let side = ui.spacing().interact_size.y;
                                let caret = if state.is_open() {
                                    icons::CARET_DOWN
                                } else {
                                    icons::CARET_RIGHT
                                };
                                if ui
                                    .add(
                                        egui::Button::new(caret)
                                            .frame(false)
                                            .min_size(egui::vec2(side * 0.6, side)),
                                    )
                                    .on_hover_text("Details")
                                    .clicked()
                                {
                                    state.toggle(ui);
                                }
                                let side = crate::app::mobile::touch_square(ui);
                                let padding = ui.spacing().button_padding.x;
                                let label_width = egui::WidgetText::from("Load")
                                    .into_galley(ui, None, f32::INFINITY, egui::TextStyle::Button)
                                    .size()
                                    .x;
                                let load_width = (side * 1.5).max(label_width + 2.0 * padding);
                                let title_width =
                                    ui.available_width() - load_width - ui.spacing().item_spacing.x;
                                ui.vertical(|ui| {
                                    ui.set_max_width(title_width);
                                    let title = egui::Label::new(
                                        egui::RichText::new(game.title).strong().size(16.0),
                                    );
                                    if state.is_open() {
                                        ui.add(title.wrap());
                                    } else {
                                        ui.add(title.truncate());
                                    }
                                    ui.small(game.tag_str());
                                });
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let load = egui::Button::new("Load")
                                            .min_size(egui::vec2(load_width, 0.0));
                                        if ui.add(load).clicked() {
                                            self.events.load_rom_data(game.data())
                                        }
                                    },
                                );
                            });
                            state.show_body_indented(&header.response, ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!("by {}", game.author))
                                            .italics(),
                                    );
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "• {} {}",
                                            icons::SCALES,
                                            game.license
                                        ))
                                        .weak(),
                                    );
                                });
                                ui.separator();
                                ui.label(game.description);
                                ui.add_space(8.0);
                                game.show_links(ui);
                            });
                        });
                    }
                })
                .response
            })
            .inner
    }
}

pub fn details_id(game: &str) -> egui::Id {
    egui::Id::new(("homebrew-details", game))
}

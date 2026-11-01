use crate::app::events::AppEventQueue;
use crate::app::ui_state::settings::{Settings, console_name, other_console, plays_on_both};
use crate::homebrew::HOMEBREW_GAMES;
use crate::icons;
use egui::{Response, ScrollArea, Ui, Widget};

pub struct HomebrewList<'a> {
    events: &'a mut AppEventQueue,
    settings: &'a mut Settings,
    link: bool,
    button_width: f32,
}

pub fn action_width(ui: &Ui, labels: &[&str]) -> f32 {
    let padding = ui.spacing().button_padding.x;
    labels
        .iter()
        .map(|&label| {
            egui::WidgetText::from(label)
                .into_galley(ui, None, f32::INFINITY, egui::TextStyle::Button)
                .size()
                .x
                + 2.0 * padding
        })
        .fold(crate::app::mobile::touch_square(ui) * 1.5, f32::max)
}

impl<'a> HomebrewList<'a> {
    pub fn new(events: &'a mut AppEventQueue, settings: &'a mut Settings) -> Self {
        Self {
            events,
            settings,
            link: false,
            button_width: 0.0,
        }
    }

    pub fn button_width(mut self, width: f32) -> Self {
        self.button_width = width;
        self
    }

    pub fn linking(mut self) -> Self {
        self.link = true;
        self
    }

    pub fn rows(self, ui: &mut Ui) -> Response {
        let action = if self.link { "Link" } else { "Load" };
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
                        let load_width = action_width(ui, &[action]).max(self.button_width);
                        let title_width =
                            ui.available_width() - load_width - ui.spacing().item_spacing.x;
                        ui.vertical(|ui| {
                            ui.set_max_width(title_width);
                            ui.style_mut().interaction.selectable_labels = false;
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
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let load =
                                egui::Button::new(action).min_size(egui::vec2(load_width, 0.0));
                            if ui.add(load).clicked() {
                                if self.link {
                                    self.events
                                        .link_game(game.data(), Some(game.title.to_owned()));
                                } else {
                                    self.events.load_rom_data(game.data());
                                }
                            }
                        });
                    });
                    if !self.link && plays_on_both(game.cgb_mode) {
                        let model =
                            other_console(self.settings.console_for(game.rom_key, game.cgb_mode));
                        let row = header.response.interact(egui::Sense::click());
                        row.context_menu(|ui| {
                            if ui
                                .button(format!("Play on {}", console_name(model)))
                                .clicked()
                            {
                                self.settings.set_game_console(game.rom_key, model);
                                self.events.load_rom_data(game.data());
                            }
                        });
                    }
                    state.show_body_indented(&header.response, ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(format!("by {}", game.author)).italics());
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
    }
}

impl Widget for HomebrewList<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        ScrollArea::vertical().show(ui, |ui| self.rows(ui)).inner
    }
}

pub fn details_id(game: &str) -> egui::Id {
    egui::Id::new(("homebrew-details", game))
}

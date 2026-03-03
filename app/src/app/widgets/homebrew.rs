use crate::app::settings::Settings;
use crate::emulator::Emulator;
use crate::homebrew::HOMEBREW_GAMES;
use crate::icons;
use citrine_gb::rom::Rom;
use egui::{Response, ScrollArea, Ui, Widget};

pub struct HomebrewList<'a> {
    emulator: &'a mut Emulator,
    settings: &'a mut Settings,
}

impl<'a> HomebrewList<'a> {
    pub fn new(emulator: &'a mut Emulator, settings: &'a mut Settings) -> Self {
        Self { emulator, settings }
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

                            let id =
                                ui.make_persistent_id(format!("homebrew-collapsible-{}", game.id));
                            egui::collapsing_header::CollapsingState::load_with_default_open(
                                ui.ctx(),
                                id,
                                false,
                            )
                            .show_header(ui, |ui| {
                                ui.label(egui::RichText::new(game.title).strong().size(16.0));

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.button("Load").clicked() {
                                            let rom = Rom::new(&game.data());
                                            #[cfg(not(target_arch = "wasm32"))]
                                            let _ = self.emulator.load_rom(&rom, None);
                                            #[cfg(target_arch = "wasm32")]
                                            let _ = self.emulator.load_rom(&rom);
                                            self.settings.dirty = true;
                                        }
                                    },
                                );
                            })
                            .body(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!("By {}", game.author))
                                            .italics(),
                                    );

                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.small(game.tag_str());
                                        },
                                    );
                                });
                                ui.separator();
                                ui.label(game.description);
                                ui.add_space(8.0);
                                ui.small(format!("{} {}", icons::SCALES, game.license));
                            });
                        });
                    }
                })
                .response
            })
            .inner
    }
}

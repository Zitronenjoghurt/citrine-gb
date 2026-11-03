use super::kit::{self, Row};
use super::sheet;
use crate::app::Citrine;
use crate::app::ui_state::settings::{console_name, other_console, plays_on_both};
use crate::homebrew::{HOMEBREW_GAMES, HomebrewGame};
use crate::icons;
use egui::{Context, Id, RichText, Ui};

const INFO_SIDE: f32 = 48.0;

enum Pick {
    Start(usize),
    Info(usize),
}

pub fn is_running(game: &HomebrewGame, running: Option<&str>) -> bool {
    running.is_some_and(|key| key.eq_ignore_ascii_case(game.rom_key))
}

impl Citrine {
    pub(super) fn homebrew_page(&mut self, ui: &mut Ui) {
        let running = self
            .emulator
            .gb
            .cartridge
            .has_rom_loaded
            .then(|| self.emulator.rom_key())
            .flatten()
            .map(str::to_owned);
        let mut pick = None;
        kit::place(ui, "homebrew_page", |ui| {
            ui.add_space(8.0);
            kit::list(ui, |list| {
                for (index, game) in HOMEBREW_GAMES.iter().enumerate() {
                    let tags = game.tag_str();
                    let mut row = Row::new(game.title).detail(&tags).trailing(icons::INFO);
                    if is_running(game, running.as_deref()) {
                        row = row.value(format!("{} Resume", icons::PLAY)).accent();
                    }
                    let response = list.row(row);
                    if response.clicked() {
                        let on_info = response
                            .interact_pointer_pos()
                            .is_some_and(|at| at.x > response.rect.right() - INFO_SIDE);
                        pick = Some(if on_info {
                            Pick::Info(index)
                        } else {
                            Pick::Start(index)
                        });
                    }
                }
            });
        });
        match pick {
            Some(Pick::Start(index)) => self.start_homebrew(index, running.as_deref()),
            Some(Pick::Info(index)) => {
                self.mobile.homebrew_info = Some(index);
                self.release_touch();
            }
            None => {}
        }
        if let Some(index) = self.mobile.homebrew_info {
            self.homebrew_sheet(ui.ctx(), index, running.as_deref());
        }
    }

    fn start_homebrew(&mut self, index: usize, running: Option<&str>) {
        let game = &HOMEBREW_GAMES[index];
        if is_running(game, running) {
            self.resume_game();
        } else {
            self.events.load_rom_data(game.data());
        }
    }

    fn homebrew_sheet(&mut self, ctx: &Context, index: usize, running: Option<&str>) {
        let game = &HOMEBREW_GAMES[index];
        let other = plays_on_both(game.cgb_mode)
            .then(|| other_console(self.ui.settings.console_for(game.rom_key, game.cgb_mode)));
        let shown = sheet::show(ctx, Id::new("homebrew_info"), game.title, |ui| {
            sheet::note(
                ui,
                &format!("By {}\n{} {}", game.author, icons::SCALES, game.license),
            );
            sheet::inset(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(game.description).size(15.0)).wrap());
            });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.add_space(sheet::INSET);
                for link in game.links {
                    let label = format!("{} {}", link.kind.icon(), link.kind.label());
                    if kit::pill(ui, &label, false).clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(link.url));
                    }
                }
            });
            ui.add_space(14.0);
            let start = if is_running(game, running) {
                "Resume"
            } else {
                "Start"
            };
            let mut chosen = None;
            if kit::primary_button(ui, start) {
                chosen = Some(None);
            }
            if let Some(model) = other {
                ui.add_space(8.0);
                let label = format!("Play on {}", console_name(model));
                if kit::list(ui, |list| list.row(Row::new(&label).no_chevron()).clicked()) {
                    chosen = Some(Some(model));
                }
            }
            chosen
        });
        if let Some(model) = shown.inner {
            if let Some(model) = model {
                self.ui.settings.set_game_console(game.rom_key, model);
                self.events.load_rom_data(game.data());
            } else {
                self.start_homebrew(index, running);
            }
            self.mobile.homebrew_info = None;
        } else if shown.closed {
            self.mobile.homebrew_info = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_running_game_is_found_by_its_rom_key() {
        let game = &HOMEBREW_GAMES[0];
        assert!(is_running(game, Some(&game.rom_key.to_uppercase())));
        assert!(!is_running(game, Some("0000")));
        assert!(!is_running(game, None));
    }
}

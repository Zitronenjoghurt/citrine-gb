use crate::icons;

include!(concat!(env!("OUT_DIR"), "/homebrew.rs"));

impl LinkKind {
    pub fn icon(&self) -> &'static str {
        match self {
            LinkKind::Official => icons::BUILDING_OFFICE,
            LinkKind::Itch => icons::GAME_CONTROLLER,
            LinkKind::Github => icons::GITHUB_LOGO,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            LinkKind::Official => "Website",
            LinkKind::Itch => "itch.io",
            LinkKind::Github => "GitHub",
        }
    }
}

impl HomebrewGame {
    pub fn show_links(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for link in self.links {
                if ui
                    .button(format!("{} {}", link.kind.icon(), link.kind.label()))
                    .on_hover_text(link.url)
                    .clicked()
                {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(link.url));
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb::gb::joypad::JoypadState;
    use citrine_gb::gb::{GameBoy, GbModel};
    use citrine_gb::rom::Rom;
    use std::collections::HashSet;

    const INDEX: &str = include_str!("../homebrew/index.toml");

    fn rom_files() -> Vec<String> {
        std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/homebrew"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                matches!(
                    path.extension().and_then(|e| e.to_str()),
                    Some("gb" | "gbc")
                )
            })
            .map(|path| path.file_stem().unwrap().to_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn every_index_entry_has_its_rom_and_every_rom_its_entry() {
        let entries = INDEX.lines().filter(|line| line.starts_with('[')).count();
        let roms = rom_files();
        assert_eq!(entries, HOMEBREW_GAMES.len());
        assert_eq!(roms.len(), HOMEBREW_GAMES.len());
        for game in HOMEBREW_GAMES {
            assert!(roms.iter().any(|rom| rom == game.id), "{}", game.id);
        }
    }

    #[test]
    fn games_sort_by_title_ignoring_case_with_micro_as_m() {
        let titles: Vec<&str> = HOMEBREW_GAMES.iter().map(|game| game.title).collect();
        assert_eq!(
            titles,
            [
                "Adjustris",
                "Dawn Will Come",
                "Libbet and the Magic Floor",
                "\u{b5}City",
                "Petris",
                "Porklike",
                "Postie",
                "Shock Lobster",
                "Sushi Nights",
                "Tobu Tobu Girl",
                "Tobu Tobu Girl Deluxe",
            ]
        );
    }

    #[test]
    fn every_game_names_its_author_licence_and_source() {
        for game in HOMEBREW_GAMES {
            assert!(!game.title.trim().is_empty(), "{}", game.id);
            assert!(!game.author.trim().is_empty(), "{}", game.id);
            assert!(!game.description.trim().is_empty(), "{}", game.id);
            assert!(!game.license.trim().is_empty(), "{}", game.id);
            assert!(!game.tags.is_empty(), "{}", game.id);
            assert!(!game.links.is_empty(), "{}", game.id);
            for link in game.links {
                assert!(
                    link.url.starts_with("https://"),
                    "{}: {}",
                    game.id,
                    link.url
                );
            }
        }
    }

    fn press_for(frame: usize) -> JoypadState {
        match (frame / 20) % 12 {
            1 => JoypadState::START,
            3 | 9 => JoypadState::A,
            5 => JoypadState::RIGHT,
            7 => JoypadState::LEFT,
            8 => JoypadState::UP | JoypadState::A,
            10 => JoypadState::DOWN,
            _ => JoypadState::empty(),
        }
    }

    fn pictures_in_second_quarter_minute(game: &HomebrewGame) -> usize {
        let rom = Rom::new(&game.data());
        let mut gb = GameBoy::new_empty(GbModel::for_rom(&rom).unwrap());
        gb.load_rom(&rom).unwrap();
        let mut pictures = HashSet::new();
        let mut held = JoypadState::empty();
        for frame in 0..1800 {
            let press = press_for(frame);
            gb.joypad.release(held & !press);
            gb.joypad.press(press);
            held = press;
            gb.run_frame();
            if frame >= 900 && frame % 30 == 0 {
                pictures.insert(gb.frame().as_slice().to_vec());
            }
        }
        pictures.len()
    }

    #[test]
    fn every_game_boots_and_keeps_drawing_through_its_first_half_minute() {
        let per_thread = HOMEBREW_GAMES.len().div_ceil(4);
        std::thread::scope(|scope| {
            for games in HOMEBREW_GAMES.chunks(per_thread) {
                scope.spawn(move || {
                    for game in games {
                        let pictures = pictures_in_second_quarter_minute(game);
                        assert!(pictures > 1, "{} showed one picture after 15 s", game.id);
                    }
                });
            }
        });
    }
}

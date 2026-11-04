use crate::icons;

pub mod loader;
#[cfg(test)]
mod storage;

include!(concat!(env!("OUT_DIR"), "/homebrew.rs"));

impl ShelfConsole {
    pub fn label(&self) -> &'static str {
        match self {
            ShelfConsole::GameBoy => "Game Boy",
            ShelfConsole::GameBoyAdvance => "Game Boy Advance",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            ShelfConsole::GameBoy => icons::DEVICE_MOBILE,
            ShelfConsole::GameBoyAdvance => icons::GAME_CONTROLLER,
        }
    }
}

pub fn shelves() -> Vec<(ShelfConsole, Vec<(usize, &'static HomebrewGame)>)> {
    let mut shelves: Vec<(ShelfConsole, Vec<(usize, &'static HomebrewGame)>)> = Vec::new();
    for (index, game) in HOMEBREW_GAMES.iter().enumerate() {
        match shelves.last_mut() {
            Some((console, games)) if *console == game.console => games.push((index, game)),
            _ => shelves.push((game.console, vec![(index, game)])),
        }
    }
    shelves
}

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
    use citrine_gb::console::Keys;
    use citrine_gb::console::{ConsoleKind, detect};
    use citrine_gb::gb::{GameBoy, GbModel};
    use citrine_gb::rom::Rom;
    use citrine_gb::rom::id::RomId;
    use std::collections::HashSet;

    const INDEX: &str = include_str!("../homebrew/index.toml");

    fn rom_files() -> Vec<String> {
        let mut roms: Vec<String> =
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
                .collect();
        if cfg!(feature = "gba") {
            roms.extend(gba_files().into_iter().map(|(id, _)| id));
        }
        roms
    }

    fn gba_files() -> Vec<(String, std::path::PathBuf)> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("homebrew/gba");
        std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter_map(|path| {
                let name = path.file_name()?.to_str()?;
                Some((name.strip_suffix(".gba.br")?.to_string(), path.clone()))
            })
            .collect()
    }

    fn game_boy_games() -> impl Iterator<Item = &'static HomebrewGame> {
        HOMEBREW_GAMES
            .iter()
            .filter(|game| game.console == ShelfConsole::GameBoy)
    }

    #[test]
    fn every_index_entry_has_its_rom_and_every_rom_its_entry() {
        let mut entries = INDEX.lines().filter(|line| line.starts_with('[')).count();
        if !cfg!(feature = "gba") {
            entries -= INDEX
                .lines()
                .filter(|line| line.starts_with("sha1 = "))
                .count();
        }
        let roms = rom_files();
        assert_eq!(entries, HOMEBREW_GAMES.len());
        assert_eq!(roms.len(), HOMEBREW_GAMES.len());
        for game in HOMEBREW_GAMES {
            assert!(roms.iter().any(|rom| rom == game.id), "{}", game.id);
        }
    }

    #[test]
    fn games_sort_by_title_ignoring_case_with_micro_as_m() {
        let titles: Vec<&str> = game_boy_games().map(|game| game.title).collect();
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
                "Snake",
                "Sushi Nights",
                "Tobu Tobu Girl",
                "Tobu Tobu Girl Deluxe",
            ]
        );
    }

    #[cfg(feature = "gba")]
    #[test]
    fn game_boy_advance_games_follow_on_their_own_shelf_by_title() {
        let shelves = shelves();
        let consoles: Vec<ShelfConsole> = shelves.iter().map(|shelf| shelf.0).collect();
        assert_eq!(
            consoles,
            [ShelfConsole::GameBoy, ShelfConsole::GameBoyAdvance]
        );
        let titles: Vec<&str> = shelves[1].1.iter().map(|(_, game)| game.title).collect();
        assert_eq!(
            titles,
            [
                "CASCADE7",
                "GBArcade",
                "Hyperspace Roll",
                "Nonogram Advance",
                "Pentris",
                "The Hat Chooses the Wizard",
            ]
        );
        for (index, game) in shelves.iter().flat_map(|shelf| &shelf.1) {
            assert_eq!(HOMEBREW_GAMES[*index].id, game.id);
        }
    }

    #[cfg(feature = "gba")]
    #[test]
    fn every_game_boy_advance_file_unpacks_to_the_upstream_rom_with_its_licence() {
        use citrine_gb::console::Console;
        let files = gba_files();
        assert_eq!(files.len(), 6);
        for (id, path) in files {
            let game = HOMEBREW_GAMES.iter().find(|game| game.id == id).unwrap();
            assert_eq!(game.console, ShelfConsole::GameBoyAdvance, "{id}");
            let stored = std::fs::read(&path).unwrap();
            let mut data = Vec::new();
            std::io::Read::read_to_end(
                &mut brotli::Decompressor::new(stored.as_slice(), 4096),
                &mut data,
            )
            .unwrap();
            assert_eq!(game.embedded().as_deref(), Some(data.as_slice()), "{id}");
            let rom_id = RomId::new(&data);
            assert_eq!(rom_id.sha1, game.sha1, "{id}");
            assert_eq!(data.len(), game.size, "{id}");
            assert_eq!(rom_id.sha256_hex_string(), game.rom_key, "{id}");
            let gba = citrine_gb::gba::machine::Gba::new(&data);
            assert_eq!(gba.rom_id().sha256_hex_string(), game.rom_key, "{id}");
            for file_name in [Some("x.gba"), None] {
                assert_eq!(detect(&data, file_name), Some(ConsoleKind::Gba), "{id}");
            }
            let licence = path.with_file_name(format!("{id}.LICENSE.txt"));
            let licence = std::fs::read_to_string(&licence).unwrap();
            assert!(licence.starts_with(game.title), "{id}");
            assert!(
                licence.contains("Permission is hereby granted")
                    || licence.contains("Mozilla Public License"),
                "{id}"
            );
        }
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

    #[test]
    fn the_link_cable_games_are_tagged_for_two_players() {
        let linked: Vec<&str> = HOMEBREW_GAMES
            .iter()
            .filter(|game| game.tags.contains(&"2-player link"))
            .map(|game| game.id)
            .collect();
        assert_eq!(linked, ["petris", "snake"]);
    }

    #[test]
    fn the_shelf_knows_each_rom_key_and_console_flag() {
        for game in game_boy_games() {
            let header = Rom::new(&game.embedded().unwrap()).header().unwrap();
            assert_eq!(game.rom_key, header.sha256_hex_string(), "{}", game.id);
            assert_eq!(game.cgb_mode, header.cgb_mode, "{}", game.id);
        }
    }

    #[test]
    fn every_homebrew_rom_keeps_its_hashes_and_reads_as_a_game_boy_game() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("homebrew");
        let mut roms = 0;
        for file in std::fs::read_dir(dir).unwrap() {
            let path = file.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if !name.ends_with(".gb") && !name.ends_with(".gbc") {
                continue;
            }
            let data = std::fs::read(&path).unwrap();
            let id = RomId::new(&data);
            let game = HOMEBREW_GAMES
                .iter()
                .find(|game| game.rom_key == id.sha256_hex_string())
                .unwrap_or_else(|| panic!("{name} has a new storage key"));
            assert_eq!(game.sha1, id.sha1, "{name}");
            assert_eq!(Rom::new(&data).header().unwrap().id(), id, "{name}");
            for file_name in [Some(name.as_str()), None] {
                let kind = detect(&data, file_name);
                assert!(
                    matches!(kind, Some(ConsoleKind::Gb | ConsoleKind::Gbc)),
                    "{name} {file_name:?} reads as {kind:?}"
                );
            }
            roms += 1;
        }
        assert_eq!(roms, game_boy_games().count());
    }

    #[test]
    fn games_for_both_consoles_also_play_on_a_game_boy() {
        for game in game_boy_games()
            .filter(|game| crate::app::ui_state::settings::plays_on_both(game.cgb_mode))
        {
            let pictures = pictures_in_second_quarter_minute(game, GbModel::Dmg);
            assert!(pictures > 1, "{} showed one picture on Game Boy", game.id);
        }
    }

    fn press_for(frame: usize) -> Keys {
        match (frame / 20) % 12 {
            1 => Keys::START,
            3 | 9 => Keys::A,
            5 => Keys::RIGHT,
            7 => Keys::LEFT,
            8 => Keys::UP | Keys::A,
            10 => Keys::DOWN,
            _ => Keys::empty(),
        }
    }

    fn pictures_in_second_quarter_minute(game: &HomebrewGame, model: GbModel) -> usize {
        let rom = Rom::new(&game.embedded().unwrap());
        let mut gb = GameBoy::new_empty(model);
        gb.load_rom(&rom).unwrap();
        let mut pictures = HashSet::new();
        for frame in 0..1800 {
            gb.set_keys(press_for(frame));
            gb.run_frame();
            if frame >= 900 && frame % 30 == 0 {
                pictures.insert(gb.frame().as_slice().to_vec());
            }
        }
        pictures.len()
    }

    #[test]
    fn every_game_boots_and_keeps_drawing_through_its_first_half_minute() {
        let games: Vec<&HomebrewGame> = game_boy_games().collect();
        let per_thread = games.len().div_ceil(4);
        std::thread::scope(|scope| {
            for games in games.chunks(per_thread) {
                scope.spawn(move || {
                    for game in games {
                        let model = GbModel::for_rom(&Rom::new(&game.embedded().unwrap())).unwrap();
                        let pictures = pictures_in_second_quarter_minute(game, model);
                        assert!(pictures > 1, "{} showed one picture after 15 s", game.id);
                    }
                });
            }
        });
    }
}

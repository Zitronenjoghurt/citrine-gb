use citrine_gb_gamedata::{Game, Version, identify};
use std::path::PathBuf;

fn rom_dir() -> PathBuf {
    std::env::var_os("CITRINE_ROMS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").expect("home")).join("roms/gb"))
}

pub(crate) fn rom_of(version: Version) -> Option<(Vec<u8>, Game)> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(rom_dir())
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            ["pok", "red", "blue", "gold", "silver"]
                .iter()
                .any(|word| name.contains(word))
        })
        .collect();
    paths.sort();
    paths.into_iter().find_map(|path| {
        let data = std::fs::read(&path).ok()?;
        let game = identify(&data)?;
        (game.version == version).then_some((data, game))
    })
}

pub(crate) fn save_from(var: &str) -> Option<Vec<u8>> {
    std::fs::read(std::env::var_os(var)?).ok()
}

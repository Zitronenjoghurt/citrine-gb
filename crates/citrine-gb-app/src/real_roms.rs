use citrine_gb_gamedata::{Game, Version, identify};
use citrine_gb_test_roms as test_roms;
use std::path::PathBuf;

pub(crate) fn rom_dir() -> PathBuf {
    std::env::var_os("CITRINE_ROMS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").expect("home")).join("roms/gb"))
}

pub(crate) fn rom_of(test: &str, version: Version) -> Option<(Vec<u8>, Game)> {
    let dir = test_roms::dir(test, rom_dir())?;
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
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
    let found = paths.into_iter().find_map(|path| {
        let data = std::fs::read(&path).ok()?;
        let game = identify(&data)?;
        (game.version == version).then_some((data, game))
    });
    test_roms::found(
        test,
        format_args!("Pokémon {version:?} in {}", dir.display()),
        found,
    )
}

pub(crate) fn save_from(test: &str, var: &str) -> Option<Vec<u8>> {
    test_roms::read(test, test_roms::env_path(test, var)?)
}

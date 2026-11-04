use super::*;

#[test]
fn a_tree_uri_shows_as_its_folder_path() {
    let tree =
        "content://com.android.externalstorage.documents/tree/primary%3ADownload%2FGame%20Boy";
    assert_eq!(folder_label(tree), "Download/Game Boy");
    let root = "content://com.android.externalstorage.documents/tree/primary%3A";
    assert_eq!(folder_label(root), "Phone storage");
}

#[test]
fn game_names_drop_the_extension_in_any_case() {
    assert_eq!(game_name("Tobu Tobu Girl.gb"), "Tobu Tobu Girl");
    assert_eq!(game_name("Petris.GBC"), "Petris");
    assert_eq!(game_name("readme.txt"), "readme.txt");
    assert_eq!(game_name("Tobu.gbcx"), "Tobu.gbcx");
}

#[test]
fn gba_games_drop_their_extension_too() {
    assert_eq!(game_name("Pokemon Emerald.GBA"), "Pokemon Emerald");
}

#[test]
fn search_matches_any_part_of_the_name() {
    let games = [
        FolderGame {
            name: "Adjustris".into(),
            uri: "a".into(),
        },
        FolderGame {
            name: "Dawn Will Come".into(),
            uri: "b".into(),
        },
    ];
    assert_eq!(matching(&games, "will").len(), 1);
    assert_eq!(matching(&games, "  ").len(), 2);
}

#[test]
fn a_plain_folder_lists_its_roms_two_levels_deep() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("a").join("b");
    let too_deep = nested.join("c");
    std::fs::create_dir_all(&too_deep).unwrap();
    std::fs::write(dir.path().join("Petris.gbc"), b"rom").unwrap();
    std::fs::write(nested.join("Adjustris.gb"), b"rom").unwrap();
    std::fs::write(too_deep.join("Hidden.gb"), b"rom").unwrap();
    std::fs::write(dir.path().join("notes.txt"), b"text").unwrap();
    let listing = list_dir(&dir.path().to_string_lossy());
    assert!(listing.readable);
    let names: Vec<&str> = listing
        .games
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(names, ["Adjustris.gb", "Petris.gbc"]);
    assert_eq!(folder_label("/home/clara/roms"), "roms");
    assert!(!list_dir("/no/such/folder").readable);
}

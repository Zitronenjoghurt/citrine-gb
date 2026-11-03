use super::*;
use std::path::PathBuf;

fn entry(path: PathBuf, title: &str) -> RecentRom {
    RecentRom {
        path,
        title: title.to_owned(),
        last_played: 0,
        rom_key: None,
    }
}

#[test]
fn the_game_now_playing_is_not_listed_again() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first.gb");
    let second = dir.path().join("second.gb");
    std::fs::write(&first, b"rom").unwrap();
    std::fs::write(&second, b"rom").unwrap();
    let entries = [entry(first.clone(), "First"), entry(second, "Second")];
    let today = jiff::civil::date(2026, 11, 2);
    let games = recent_games(&entries, (Some(&first), None), today, |path| {
        !path.is_file()
    });
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].name, "Second");
    assert!(!games[0].gone);
}

#[test]
fn a_missing_file_shows_as_gone_with_remove() {
    let dir = tempfile::tempdir().unwrap();
    let entries = [entry(dir.path().join("moved.gb"), "Moved")];
    let games = recent_games(
        &entries,
        (None, None),
        jiff::civil::date(2026, 11, 2),
        |path| !path.is_file(),
    );
    assert!(games[0].gone);
    assert_eq!(games[0].detail, "File not found");
}

#[test]
fn a_kept_copy_of_the_game_now_playing_is_not_listed_again() {
    let entries = [entry(PathBuf::from("abc123.gbc"), "Copy")];
    let today = jiff::civil::date(2026, 11, 2);
    let games = recent_games(&entries, (None, Some("abc123")), today, |_| false);
    assert!(games.is_empty());
}

use super::HOMEBREW_GAMES;
use citrine_gb_gameinfo::game_name as table;

pub use table::*;

fn shelf_title(sha1: &[u8; 20]) -> Option<&'static str> {
    HOMEBREW_GAMES
        .iter()
        .find(|game| game.sha1 == *sha1)
        .map(|game| game.title)
}

pub fn known_name(sha1: &[u8; 20]) -> Option<&'static str> {
    table::known_name(sha1).or_else(|| shelf_title(sha1))
}

pub fn name_or<'a>(sha1: &[u8; 20], title: &'a str) -> &'a str {
    known_name(sha1).unwrap_or(title)
}

pub fn game_name(sha1: &[u8; 20], file_name: Option<&str>, header_title: &str) -> String {
    match shelf_title(sha1) {
        Some(title) => title.to_string(),
        None => table::game_name(sha1, file_name, header_title),
    }
}

#[cfg(feature = "nds")]
pub fn known_ds_name(sha1: &[u8; 20], game_code: [u8; 4]) -> Option<&'static str> {
    shelf_title(sha1).or_else(|| table::known_ds_name(game_code))
}

#[cfg(feature = "nds")]
pub fn ds_game_name(
    sha1: &[u8; 20],
    game_code: [u8; 4],
    banner_title: Option<&str>,
    file_name: Option<&str>,
    header_title: &str,
) -> String {
    match shelf_title(sha1) {
        Some(title) => title.to_string(),
        None => table::ds_game_name(game_code, banner_title, file_name, header_title),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shelf_game_gets_its_shelf_title() {
        for game in HOMEBREW_GAMES {
            assert_eq!(game_name(&game.sha1, Some("x.gb"), "IGNORED"), game.title);
        }
    }

    #[cfg(feature = "nds")]
    #[test]
    fn a_shelf_game_gets_its_shelf_title_though_every_homebrew_shares_one_code() {
        use crate::homebrew::ShelfConsole;
        let shelf: Vec<_> = HOMEBREW_GAMES
            .iter()
            .filter(|game| game.console == ShelfConsole::NintendoDs)
            .collect();
        assert!(!shelf.is_empty());
        for game in shelf {
            assert_eq!(known_ds_name(&game.sha1, *b"####"), Some(game.title));
            assert_eq!(
                ds_game_name(&game.sha1, *b"####", Some("Banner"), None, "HOMEBREW"),
                game.title
            );
        }
        assert_eq!(known_ds_name(&[0; 20], *b"####"), None);
    }
}

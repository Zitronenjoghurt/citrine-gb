use super::{from_file_name, from_header};

include!(concat!(env!("OUT_DIR"), "/ds_names.rs"));

pub fn known_ds_name(game_code: [u8; 4]) -> Option<&'static str> {
    DS_NAMES
        .binary_search_by_key(&game_code, |entry| entry.0)
        .ok()
        .map(|index| DS_NAMES[index].1)
}

pub fn ds_game_name(
    game_code: [u8; 4],
    banner_title: Option<&str>,
    file_name: Option<&str>,
    header_title: &str,
) -> String {
    if let Some(name) = known_ds_name(game_code) {
        return name.to_string();
    }
    if let Some(name) = banner_title.map(str::trim).filter(|name| !name.is_empty()) {
        return name.to_string();
    }
    if let Some(name) = file_name
        .map(from_file_name)
        .filter(|name| !name.is_empty())
    {
        return name;
    }
    from_header(header_title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_holds_both_sources_sorted_with_tidy_names() {
        assert!(DS_NAMES.len() > 380, "{}", DS_NAMES.len());
        assert!(DS_NAMES.is_sorted_by_key(|entry| entry.0));
        for (code, name) in DS_NAMES {
            assert!(code.iter().all(u8::is_ascii_alphanumeric), "{code:?}");
            assert!(!name.is_empty());
            assert_eq!(super::super::tags::without_tags(name), *name);
        }
    }

    #[test]
    fn a_listed_game_code_gets_its_name() {
        let cases = [
            (*b"CPUE", "Pokémon Platinum"),
            (*b"CPUP", "Pokémon Platinum"),
            (*b"AMCE", "Mario Kart DS"),
            (*b"ASPE", "Super Princess Peach"),
            (*b"AB6E", "Bubble Bobble Revolution"),
            (*b"A6SE", "The Legend of Spyro : A New Beginning"),
        ];
        for (code, name) in cases {
            assert_eq!(known_ds_name(code), Some(name));
            assert_eq!(
                ds_game_name(code, Some("Banner"), Some("x.nds"), "IGNORED"),
                name
            );
        }
    }

    #[test]
    fn a_code_missing_from_the_lists_falls_back_to_the_banner_title() {
        assert_eq!(known_ds_name(*b"IPGE"), None);
        assert_eq!(
            ds_game_name(
                *b"IPGE",
                Some(" Pokémon SoulSilver Version "),
                Some("4787 - Pokemon SoulSilver (U).nds"),
                "POKEMON SLV"
            ),
            "Pokémon SoulSilver Version"
        );
    }

    #[test]
    fn without_a_banner_title_the_file_name_then_the_header_title_name_the_game() {
        assert_eq!(
            ds_game_name(
                *b"IPGE",
                Some("  "),
                Some("Pokemon SoulSilver (USA).nds"),
                "POKEMON SLV"
            ),
            "Pokemon SoulSilver"
        );
        assert_eq!(
            ds_game_name(*b"IPGE", None, None, "POKEMON SLV"),
            "Pokemon Slv"
        );
    }
}

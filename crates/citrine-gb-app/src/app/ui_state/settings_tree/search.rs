use super::rows::RowId;
use super::{GroupId, SettingsTab, Surface};
use strum::IntoEnumIterator;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub page: SettingsTab,
    pub group: GroupId,
    pub row: RowId,
}

pub fn search(query: &str, surface: Surface, dev_mode: bool) -> Vec<Hit> {
    let query = fold(query.trim());
    if query.is_empty() {
        return Vec::new();
    }
    let mut ranked: Vec<(u8, Hit)> = RowId::iter()
        .filter(|row| row.available(dev_mode) && row.surfaces().contains(&surface))
        .filter_map(|row| {
            let hit = Hit {
                page: row.page(),
                group: row.group(),
                row,
            };
            rank(row, surface, &query).map(|rank| (rank, hit))
        })
        .collect();
    ranked.sort_by_key(|(rank, hit)| (*rank, hit.page, hit.group, hit.row));
    ranked.into_iter().map(|(_, hit)| hit).collect()
}

fn rank(row: RowId, surface: Surface, query: &str) -> Option<u8> {
    let label = fold(row.label(surface));
    if label.starts_with(query) {
        Some(0)
    } else if starts_a_word(&label, query) {
        Some(1)
    } else if row
        .keywords()
        .iter()
        .any(|keyword| starts_a_word(&fold(keyword), query))
    {
        Some(2)
    } else if starts_a_word(&fold(row.group().title()), query) {
        Some(3)
    } else if starts_a_word(&fold(row.page().title()), query) {
        Some(4)
    } else {
        None
    }
}

fn starts_a_word(text: &str, query: &str) -> bool {
    text.match_indices(query).any(|(at, _)| {
        !text[..at]
            .chars()
            .next_back()
            .is_some_and(char::is_alphanumeric)
    })
}

fn fold(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'ç' => 'c',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ñ' => 'n',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            'ý' | 'ÿ' => 'y',
            '-' => ' ',
            '\u{2019}' => '\'',
            c => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_folds_case_accents_and_hyphens() {
        assert_eq!(fold("Pokéwalker"), "pokewalker");
        assert_eq!(fold("Left-handed"), "left handed");
        assert!(starts_a_word("match refresh rate", "refresh"));
        assert!(!starts_a_word("program", "ram"));
        for hit in search("screen", Surface::Phone, true) {
            assert_eq!(hit.group, hit.row.group());
            assert_eq!(hit.page, hit.group.page());
        }
    }
}

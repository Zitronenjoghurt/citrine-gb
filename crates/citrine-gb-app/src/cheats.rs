use citrine_gb::cheats::{Cheat, CheatError, Format};
use citrine_gb::console::ConsoleKind;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SavedCheat {
    pub name: String,
    pub code: String,
    pub on: bool,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct CheatBook(BTreeMap<String, Vec<SavedCheat>>);

impl CheatBook {
    pub fn list(&self, rom_key: &str) -> &[SavedCheat] {
        self.0.get(rom_key).map_or(&[], Vec::as_slice)
    }

    pub fn add(&mut self, rom_key: &str, cheat: SavedCheat) {
        self.0.entry(rom_key.to_string()).or_default().push(cheat);
    }

    pub fn insert(&mut self, rom_key: &str, index: usize, cheat: SavedCheat) {
        let list = self.0.entry(rom_key.to_string()).or_default();
        list.insert(index.min(list.len()), cheat);
    }

    pub fn replace(&mut self, rom_key: &str, index: usize, cheat: SavedCheat) -> bool {
        match self.0.get_mut(rom_key).and_then(|list| list.get_mut(index)) {
            Some(slot) if *slot != cheat => {
                *slot = cheat;
                true
            }
            _ => false,
        }
    }

    pub fn set_on(&mut self, rom_key: &str, index: usize, on: bool) -> bool {
        match self.0.get_mut(rom_key).and_then(|list| list.get_mut(index)) {
            Some(cheat) if cheat.on != on => {
                cheat.on = on;
                true
            }
            _ => false,
        }
    }

    pub fn remove(&mut self, rom_key: &str, index: usize) -> Option<SavedCheat> {
        let list = self.0.get_mut(rom_key)?;
        if index >= list.len() {
            return None;
        }
        let cheat = list.remove(index);
        if list.is_empty() {
            self.0.remove(rom_key);
        }
        Some(cheat)
    }

    pub fn turn_off_unknown(&mut self, known: &CheatBook) {
        for (rom_key, list) in &mut self.0 {
            let known = known.list(rom_key);
            for cheat in list {
                cheat.on &= known.iter().any(|old| old.on && old.code == cheat.code);
            }
        }
    }
}

pub fn formats_text(console: ConsoleKind) -> String {
    let names: Vec<&str> = Format::of(console).iter().map(|f| f.label()).collect();
    match names.as_slice() {
        [] => String::new(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
    }
}

pub fn problem(console: ConsoleKind, code: &str) -> Option<String> {
    let error = Cheat::parse(console, code).err()?;
    let formats = formats_text(console);
    let lines = code.lines().filter(|line| !line.trim().is_empty()).count();
    Some(match error {
        CheatError::Empty => format!("Type or paste a {formats} code"),
        CheatError::Unreadable { .. } if lines <= 1 => format!("Not a {formats} code"),
        CheatError::Unreadable { line } => format!("Line {line} is not a {formats} code"),
        CheatError::Unsupported => "This version can't use cheats for this console".to_string(),
    })
}

pub fn format_of(console: ConsoleKind, code: &str) -> Option<Format> {
    Cheat::parse(console, code).ok().map(|cheat| cheat.format())
}

pub fn compile(console: ConsoleKind, saved: &[SavedCheat]) -> Vec<String> {
    saved
        .iter()
        .filter(|cheat| cheat.on && Cheat::parse(console, &cheat.code).is_ok())
        .map(|cheat| cheat.code.clone())
        .collect()
}

pub fn default_name(list: &[SavedCheat]) -> String {
    (1..)
        .map(|n| format!("Cheat {n}"))
        .find(|name| list.iter().all(|cheat| cheat.name != *name))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cheat(name: &str, code: &str, on: bool) -> SavedCheat {
        SavedCheat {
            name: name.into(),
            code: code.into(),
            on,
        }
    }

    #[test]
    fn a_book_keeps_each_games_cheats_apart_and_drops_an_emptied_game() {
        let mut book = CheatBook::default();
        book.add("a", cheat("Money", "01FF47D3", true));
        book.add("b", cheat("Items", "00A-17B-C49", false));
        assert_eq!(book.list("a").len(), 1);
        assert_eq!(book.list("b")[0].name, "Items");
        assert_eq!(book.remove("a", 0).map(|c| c.name), Some("Money".into()));
        assert!(book.list("a").is_empty());
        assert_eq!(
            serde_json::to_value(&book).unwrap()["a"],
            serde_json::Value::Null
        );
    }

    #[test]
    fn an_undone_delete_puts_the_cheat_back_where_it_was() {
        let mut book = CheatBook::default();
        for name in ["One", "Two", "Three"] {
            book.add("a", cheat(name, "01FF47D3", true));
        }
        let gone = book.remove("a", 1).unwrap();
        book.insert("a", 1, gone);
        let names: Vec<_> = book.list("a").iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["One", "Two", "Three"]);
    }

    #[test]
    fn only_switched_on_codes_that_read_reach_the_console() {
        let saved = [
            cheat("On", "01FF47D3", true),
            cheat("Off", "01FF48D3", false),
            cheat("Broken", "nope", true),
        ];
        assert_eq!(compile(ConsoleKind::Gb, &saved).len(), 1);
    }

    #[test]
    fn a_code_that_does_not_read_names_the_formats_the_console_takes() {
        assert_eq!(
            problem(ConsoleKind::Gb, "nope").as_deref(),
            Some("Not a Game Genie or GameShark code")
        );
        assert_eq!(
            problem(ConsoleKind::Gbc, "01FF47D3\nnope").as_deref(),
            Some("Line 2 is not a Game Genie or GameShark code")
        );
        assert_eq!(problem(ConsoleKind::Gb, "01FF47D3"), None);
    }

    #[test]
    fn a_new_cheat_gets_the_first_free_number() {
        let list = [cheat("Cheat 1", "", true), cheat("Cheat 3", "", true)];
        assert_eq!(default_name(&list), "Cheat 2");
    }
}

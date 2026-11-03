use egui::{Event, Key, Modifiers};

const SEED: &str = " ";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImeText {
    text: Vec<char>,
    cursor: usize,
}

impl ImeText {
    #[cfg(target_os = "android")]
    pub const fn empty() -> Self {
        Self {
            text: Vec::new(),
            cursor: 0,
        }
    }

    pub fn seed() -> Self {
        Self::new(SEED, SEED.chars().count())
    }

    pub fn seed_text() -> &'static str {
        SEED
    }

    pub fn new(text: &str, cursor: usize) -> Self {
        let text: Vec<char> = text.chars().collect();
        let cursor = cursor.min(text.len());
        Self { text, cursor }
    }

    pub fn from_utf16(text: &str, cursor_utf16: i32) -> Self {
        let mut units = 0;
        let mut cursor = 0;
        for c in text.chars() {
            if units >= cursor_utf16.max(0) as usize {
                break;
            }
            units += c.len_utf16();
            cursor += 1;
        }
        Self::new(text, cursor)
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

fn tap(key: Key, events: &mut Vec<Event>) {
    for pressed in [true, false] {
        events.push(Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
    }
}

fn walk(from: usize, to: usize, events: &mut Vec<Event>) {
    let key = if to > from {
        Key::ArrowRight
    } else {
        Key::ArrowLeft
    };
    for _ in 0..from.abs_diff(to) {
        tap(key, events);
    }
}

fn type_text(text: &[char], events: &mut Vec<Event>) {
    let mut line = String::new();
    for &c in text {
        if c == '\n' {
            if !line.is_empty() {
                events.push(Event::Text(std::mem::take(&mut line)));
            }
            tap(Key::Enter, events);
        } else if !c.is_control() {
            line.push(c);
        }
    }
    if !line.is_empty() {
        events.push(Event::Text(line));
    }
}

pub fn edits(old: &ImeText, new: &ImeText) -> Vec<Event> {
    let prefix = old
        .text
        .iter()
        .zip(&new.text)
        .take_while(|(a, b)| a == b)
        .count();
    let room = old.text.len().min(new.text.len()) - prefix;
    let suffix = old.text[prefix..]
        .iter()
        .rev()
        .zip(new.text[prefix..].iter().rev())
        .take(room)
        .take_while(|(a, b)| a == b)
        .count();
    let old_end = old.text.len() - suffix;
    let new_end = new.text.len() - suffix;

    let mut events = Vec::new();
    if old_end == prefix && new_end == prefix {
        walk(old.cursor, new.cursor, &mut events);
        return events;
    }
    walk(old.cursor, old_end, &mut events);
    for _ in prefix..old_end {
        tap(Key::Backspace, &mut events);
    }
    type_text(&new.text[prefix..new_end], &mut events);
    walk(new_end, new.cursor, &mut events);
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(events: &[Event]) -> Vec<String> {
        events
            .iter()
            .filter_map(|event| match event {
                Event::Key {
                    key, pressed: true, ..
                } => Some(format!("{key:?}")),
                Event::Key { .. } => None,
                Event::Text(text) => Some(format!("'{text}'")),
                _ => Some("other".into()),
            })
            .collect()
    }

    fn at_end(text: &str) -> ImeText {
        ImeText::new(text, text.chars().count())
    }

    #[test]
    fn the_seed_leaves_something_to_delete() {
        assert!(!ImeText::seed().is_empty());
        assert_eq!(ImeText::seed(), at_end(ImeText::seed_text()));
        assert!(ImeText::new("", 0).is_empty());
    }

    #[test]
    fn typing_after_the_seed_is_text() {
        let typed = edits(&ImeText::seed(), &at_end(" hi"));
        assert_eq!(keys(&typed), ["'hi'"]);
    }

    #[test]
    fn deleting_is_backspace() {
        assert_eq!(keys(&edits(&at_end(" ab"), &at_end(" a"))), ["Backspace"]);
        assert_eq!(
            keys(&edits(&at_end(" "), &at_end(""))),
            ["Backspace"],
            "deleting the seed still reaches the field"
        );
    }

    #[test]
    fn a_corrected_word_is_replaced_where_it_is() {
        let fixed = edits(&at_end(" teh "), &at_end(" the "));
        assert_eq!(
            keys(&fixed),
            ["ArrowLeft", "Backspace", "Backspace", "'he'", "ArrowRight"]
        );
    }

    #[test]
    fn enter_is_a_key_and_control_characters_drop() {
        let typed = edits(&at_end(" a"), &at_end(" a\nb\u{0}"));
        assert_eq!(keys(&typed), ["Enter", "'b'"]);
    }

    #[test]
    fn moving_the_cursor_moves_it_in_the_field() {
        let moved = edits(&at_end(" abc"), &ImeText::new(" abc", 2));
        assert_eq!(keys(&moved), ["ArrowLeft", "ArrowLeft"]);
    }

    #[test]
    fn the_cursor_counts_characters_not_utf16_units() {
        let text = " \u{1F600}a";
        assert_eq!(ImeText::from_utf16(text, 3), ImeText::new(text, 2));
        assert_eq!(ImeText::from_utf16(text, 99), at_end(text));
        assert_eq!(ImeText::from_utf16(text, -1), ImeText::new(text, 0));
    }
}

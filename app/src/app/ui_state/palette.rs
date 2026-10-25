use citrine_gb::gb::ppu::types::color::RGBA;
use citrine_gb::gb::ppu::types::theme::DmgTheme;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CustomPalette {
    pub name: String,
    pub colors: [[u8; 3]; 4],
}

impl CustomPalette {
    pub fn from_theme(name: impl Into<String>, theme: &DmgTheme) -> Self {
        let palette = theme.palette();
        Self {
            name: name.into(),
            colors: std::array::from_fn(|i| [palette[i].r(), palette[i].g(), palette[i].b()]),
        }
    }

    pub fn rgba(&self) -> [RGBA; 4] {
        std::array::from_fn(|i| {
            let [r, g, b] = self.colors[i];
            RGBA::rgb(r, g, b)
        })
    }

    pub fn to_theme(&self) -> DmgTheme {
        DmgTheme::Custom(self.rgba())
    }

    pub fn to_code(&self) -> String {
        self.colors
            .iter()
            .map(|[r, g, b]| format!("{r:02X}{g:02X}{b:02X}"))
            .collect::<Vec<_>>()
            .join("-")
    }

    pub fn parse_code(name: impl Into<String>, code: &str) -> Option<Self> {
        let parts: Vec<&str> = code
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|part| !part.is_empty())
            .collect();

        if parts.len() != 4 {
            return None;
        }

        let mut colors = [[0u8; 3]; 4];
        for (slot, part) in colors.iter_mut().zip(parts) {
            *slot = parse_hex(part)?;
        }
        Some(Self {
            name: name.into(),
            colors,
        })
    }
}

fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let byte = |at: usize| u8::from_str_radix(text.get(at..at + 2)?, 16).ok();
    match text.len() {
        6 => Some([byte(0)?, byte(2)?, byte(4)?]),
        3 => {
            let nibble = |at: usize| {
                let value = u8::from_str_radix(text.get(at..at + 1)?, 16).ok()?;
                Some(value * 17)
            };
            Some([nibble(0)?, nibble(1)?, nibble(2)?])
        }
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PaletteChoice {
    Builtin(DmgTheme),
    Custom(String),
}

#[derive(Default, Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PaletteLibrary {
    entries: Vec<CustomPalette>,
}

impl PaletteLibrary {
    pub fn entries(&self) -> &[CustomPalette] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, name: &str) -> Option<&CustomPalette> {
        self.entries.iter().find(|entry| entry.name == name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut CustomPalette> {
        self.entries.iter_mut().find(|entry| entry.name == name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    pub fn upsert(&mut self, palette: CustomPalette) {
        match self.entries.iter_mut().find(|e| e.name == palette.name) {
            Some(existing) => *existing = palette,
            None => self.entries.push(palette),
        }
    }

    pub fn remove(&mut self, name: &str) {
        self.entries.retain(|entry| entry.name != name);
    }

    pub fn rename(&mut self, from: &str, to: &str) -> bool {
        let to = to.trim();
        if to.is_empty() || (to != from && self.contains(to)) {
            return false;
        }
        match self.get_mut(from) {
            Some(entry) => {
                entry.name = to.to_string();
                true
            }
            None => false,
        }
    }

    pub fn unique_name(&self, base: &str) -> String {
        let base = base.trim();
        let base = if base.is_empty() { "Palette" } else { base };
        if !self.contains(base) {
            return base.to_string();
        }
        (2..)
            .map(|n| format!("{base} {n}"))
            .find(|name| !self.contains(name))
            .unwrap_or_else(|| base.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette(name: &str) -> CustomPalette {
        CustomPalette {
            name: name.to_string(),
            colors: [[1, 2, 3], [4, 5, 6], [7, 8, 9], [10, 11, 12]],
        }
    }

    #[test]
    fn a_code_round_trips() {
        let original = CustomPalette::from_theme("Citrine copy", &DmgTheme::Citrine);
        let code = original.to_code();
        let parsed = CustomPalette::parse_code("Citrine copy", &code).expect("parses");
        assert_eq!(parsed, original);
    }

    #[test]
    fn codes_survive_being_pasted_around() {
        let expected = [[0xF2, 0xCE, 0x44], [0, 0, 0], [0xFF, 0xFF, 0xFF], [1, 2, 3]];
        for code in [
            "F2CE44-000000-FFFFFF-010203",
            "#F2CE44 #000000 #FFFFFF #010203",
            "f2ce44, 000000, ffffff, 010203",
            "  F2CE44\n000000\nFFFFFF\n010203  ",
        ] {
            let parsed = CustomPalette::parse_code("x", code)
                .unwrap_or_else(|| panic!("should parse {code:?}"));
            assert_eq!(parsed.colors, expected, "for {code:?}");
        }
    }

    #[test]
    fn the_three_digit_shorthand_expands() {
        let parsed = CustomPalette::parse_code("x", "FFF-000-F00-0F0").expect("parses");
        assert_eq!(parsed.colors[0], [0xFF, 0xFF, 0xFF]);
        assert_eq!(parsed.colors[2], [0xFF, 0, 0]);
    }

    #[test]
    fn codes_that_are_not_four_colours_are_rejected() {
        for code in ["F2CE44-000000-FFFFFF", "F2CE44-000000-FFFFFF-010203-999999"] {
            assert!(CustomPalette::parse_code("x", code).is_none(), "{code:?}");
        }
        assert!(CustomPalette::parse_code("x", "ZZZZZZ-000000-FFFFFF-010203").is_none());
    }

    #[test]
    fn upsert_replaces_rather_than_duplicates() {
        let mut library = PaletteLibrary::default();
        library.upsert(palette("Mine"));
        let mut updated = palette("Mine");
        updated.colors[0] = [99, 99, 99];
        library.upsert(updated);

        assert_eq!(library.entries().len(), 1);
        assert_eq!(
            library.get("Mine").expect("present").colors[0],
            [99, 99, 99]
        );
    }

    #[test]
    fn duplicate_names_get_a_free_suffix() {
        let mut library = PaletteLibrary::default();
        assert_eq!(library.unique_name("Mine"), "Mine");

        library.upsert(palette("Mine"));
        assert_eq!(library.unique_name("Mine"), "Mine 2");

        library.upsert(palette("Mine 2"));
        assert_eq!(library.unique_name("Mine"), "Mine 3");
    }

    #[test]
    fn renaming_refuses_blank_and_taken_names() {
        let mut library = PaletteLibrary::default();
        library.upsert(palette("A"));
        library.upsert(palette("B"));

        assert!(!library.rename("A", "  "), "blank is refused");
        assert!(!library.rename("A", "B"), "taken is refused");
        assert!(library.rename("A", "A"), "renaming to itself is fine");
        assert!(library.rename("A", "C"));

        assert!(library.contains("C"));
        assert!(!library.contains("A"));
        assert_eq!(library.entries()[0].name, "C", "keeps its position");
    }

    #[test]
    fn an_empty_base_name_still_produces_something_usable() {
        let library = PaletteLibrary::default();
        assert_eq!(library.unique_name("   "), "Palette");
    }
}

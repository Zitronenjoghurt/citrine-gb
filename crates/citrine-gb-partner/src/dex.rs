use citrine_gb_gamedata::{BaseStats, Generation, RomTables};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub index: u8,
    pub name: String,
    pub base: BaseStats,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Dex {
    entries: Vec<Option<Entry>>,
}

impl Dex {
    pub fn from_tables(tables: &RomTables) -> Self {
        let count = match tables.game.generation() {
            Generation::One => 151,
            Generation::Two => 251,
        };
        let entries = (1..=count)
            .map(|dex| {
                let index = tables.species_for_dex(dex)?;
                Some(Entry {
                    index,
                    name: tables.species_name(index)?.to_string(),
                    base: *tables.base_stats(index)?,
                })
            })
            .collect();
        Self { entries }
    }

    pub fn get(&self, dex: u16) -> Option<&Entry> {
        let index = usize::from(dex).checked_sub(1)?;
        self.entries.get(index)?.as_ref()
    }

    #[cfg(test)]
    pub fn insert(&mut self, dex: u16, entry: Entry) {
        let index = usize::from(dex) - 1;
        if self.entries.len() <= index {
            self.entries.resize(index + 1, None);
        }
        self.entries[index] = Some(entry);
    }
}

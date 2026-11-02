use super::{CloudState, Entry, GameLedger, Ledger, ledger_name};

impl CloudState {
    pub(super) fn ledger(&mut self) -> &mut Ledger {
        self.ledger.get_or_insert_with(Ledger::new)
    }

    pub(super) fn game(&mut self, rom: &str) -> &mut GameLedger {
        self.ledger_dirty = true;
        self.ledger().games.entry(rom.to_string()).or_default()
    }

    pub(super) fn entry(&self, rom: &str, slot: &str) -> Option<&Entry> {
        self.ledger.as_ref()?.games.get(rom)?.entries.get(slot)
    }

    pub(super) fn set_entry(&mut self, rom: &str, slot: &str, entry: Option<Entry>) {
        let game = self.game(rom);
        match entry {
            Some(entry) => game.entries.insert(slot.to_string(), entry),
            None => game.entries.remove(slot),
        };
    }

    pub(super) fn device(&mut self) -> String {
        self.ledger().device.clone()
    }

    pub(super) fn load(&mut self, owner: &str) {
        let Some(store) = &self.store else {
            return;
        };
        self.ledger = store
            .read_ledger(&ledger_name(owner))
            .and_then(|bytes| postcard::from_bytes(&bytes).ok());
        self.ledger_dirty = false;
    }

    pub(super) fn save_ledger(&mut self) {
        if !self.ledger_dirty {
            return;
        }
        self.ledger_dirty = false;
        let (Some(store), Some(owner), Some(ledger)) = (&self.store, &self.owner, &self.ledger)
        else {
            return;
        };
        match postcard::to_allocvec(ledger) {
            Ok(bytes) => {
                if let Err(err) = store.write_ledger(&ledger_name(owner), &bytes) {
                    log::warn!("could not keep the cloud save list: {err}");
                }
            }
            Err(err) => log::warn!("could not encode the cloud save list: {err}"),
        }
    }

    pub(super) fn is_dirty(&self, rom: &str) -> bool {
        self.ledger
            .as_ref()
            .and_then(|l| l.games.get(rom))
            .is_some_and(|g| g.dirty)
    }

    pub(super) fn entry_title(&self, rom: &str) -> Option<&str> {
        self.ledger
            .as_ref()?
            .games
            .get(rom)
            .map(|g| g.title.as_str())
    }

    pub(super) fn title_of(&self, rom: &str) -> &str {
        self.names
            .get(rom)
            .map(String::as_str)
            .or_else(|| self.entry_title(rom))
            .unwrap_or_default()
    }
}

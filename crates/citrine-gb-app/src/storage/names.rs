use super::{BATTERY_NAME, MAX_SAVE_NAME, SaveStore, default_slot_name, snapshot_name};

impl SaveStore {
    pub fn set_snapshot_name(&self, rom_key: &str, slot: usize, name: &str) -> std::io::Result<()> {
        self.write_name(
            rom_key,
            &snapshot_name(slot),
            name,
            &default_slot_name(slot),
        )
    }

    pub fn snapshot_name(&self, rom_key: &str, slot: usize) -> Option<String> {
        self.read_name(rom_key, &snapshot_name(slot))
    }

    pub fn set_battery_name(
        &self,
        rom_key: &str,
        name: &str,
        default_name: &str,
    ) -> std::io::Result<()> {
        self.write_name(rom_key, BATTERY_NAME, name, default_name)
    }

    pub fn battery_name(&self, rom_key: &str) -> Option<String> {
        self.read_name(rom_key, BATTERY_NAME)
    }

    pub(super) fn write_name(
        &self,
        rom_key: &str,
        file: &str,
        name: &str,
        default_name: &str,
    ) -> std::io::Result<()> {
        let name = name.trim();
        if name.is_empty() || name == default_name {
            return self.delete(rom_key, file);
        }
        let trimmed: String = name.chars().take(MAX_SAVE_NAME).collect();
        self.write(rom_key, file, trimmed.as_bytes())
    }

    pub(super) fn read_name(&self, rom_key: &str, file: &str) -> Option<String> {
        let raw = self.backend.read(rom_key, file)?;
        let name = String::from_utf8(raw).ok()?;
        let name = name.trim();
        (!name.is_empty()).then(|| name.to_string())
    }
}

use super::{
    SaveStore, SnapshotInfo, compress, decode_meta, decompress, downscale, encode_meta, now_unix,
    snapshot_blob, snapshot_meta, snapshot_name, snapshot_thumb, thumb_size,
};
use citrine_gb::console::ConsoleKind;
use citrine_gb::persistence::state::state_kind;

impl SaveStore {
    pub fn store_snapshot(
        &self,
        rom_key: &str,
        slot: usize,
        state: &[u8],
        frame_rgba: &[u8],
        console: ConsoleKind,
    ) -> std::io::Result<()> {
        self.write(rom_key, &snapshot_blob(slot), state)?;
        self.write(
            rom_key,
            &snapshot_thumb(slot),
            &compress(&downscale(frame_rgba, console.video())),
        )?;
        self.write(
            rom_key,
            &snapshot_meta(slot),
            &encode_meta(now_unix(), console),
        )
    }

    pub fn load_snapshot(&self, rom_key: &str, slot: usize) -> Option<Vec<u8>> {
        self.backend.read(rom_key, &snapshot_blob(slot))
    }

    pub fn delete_snapshot(&self, rom_key: &str, slot: usize) -> std::io::Result<()> {
        self.delete(rom_key, &snapshot_name(slot))?;
        self.delete(rom_key, &snapshot_meta(slot))?;
        self.delete(rom_key, &snapshot_thumb(slot))?;
        self.delete(rom_key, &snapshot_blob(slot))
    }

    pub fn snapshot_info(&self, rom_key: &str, slot: usize) -> Option<SnapshotInfo> {
        let mut info = self.read_info(
            rom_key,
            slot,
            &snapshot_blob(slot),
            &snapshot_meta(slot),
            &snapshot_thumb(slot),
        )?;
        info.name = self.snapshot_name(rom_key, slot);
        Some(info)
    }

    pub(super) fn read_info(
        &self,
        rom_key: &str,
        slot: usize,
        blob: &str,
        meta: &str,
        thumb: &str,
    ) -> Option<SnapshotInfo> {
        if !self.backend.exists(rom_key, blob) {
            return None;
        }
        let (saved_at, console) = self
            .backend
            .read(rom_key, meta)
            .map(|b| decode_meta(&b))
            .unwrap_or_default();
        let console = console.or_else(|| self.note_console(rom_key, blob, meta, saved_at));
        let saved_at = saved_at.unwrap_or(0);
        let [width, height] = thumb_size(console);
        let thumbnail = self
            .backend
            .read(rom_key, thumb)
            .and_then(|t| decompress(&t))
            .filter(|t| t.len() == width * height * 4)
            .unwrap_or_else(|| vec![0; width * height * 4]);
        Some(SnapshotInfo {
            slot,
            saved_at,
            thumbnail,
            name: None,
            console,
        })
    }

    pub(super) fn note_console(
        &self,
        rom_key: &str,
        blob: &str,
        meta: &str,
        saved_at: Option<u64>,
    ) -> Option<ConsoleKind> {
        let console = state_kind(&self.backend.read(rom_key, blob)?)?;
        let meta_bytes = encode_meta(saved_at.unwrap_or(0), console);
        if let Err(err) = self.write(rom_key, meta, &meta_bytes) {
            log::warn!("could not note the console of {blob}: {err}");
        }
        Some(console)
    }

    pub fn used_slots(&self, rom_key: &str) -> Vec<usize> {
        self.numbered(rom_key, "slot")
    }

    pub(super) fn numbered(&self, rom_key: &str, prefix: &str) -> Vec<usize> {
        let mut ids: Vec<usize> = self
            .backend
            .list(rom_key)
            .iter()
            .filter_map(|name| {
                name.strip_prefix(prefix)
                    .and_then(|rest| rest.strip_suffix(".snap"))
                    .and_then(|n| n.parse().ok())
            })
            .collect();
        ids.sort_unstable();
        ids
    }

    pub fn snapshot_infos(&self, rom_key: &str) -> Vec<SnapshotInfo> {
        self.used_slots(rom_key)
            .into_iter()
            .filter_map(|slot| self.snapshot_info(rom_key, slot))
            .collect()
    }

    pub fn next_free_slot(&self, rom_key: &str) -> usize {
        let used = self.used_slots(rom_key);
        (0..).find(|slot| !used.contains(slot)).unwrap_or(0)
    }
}

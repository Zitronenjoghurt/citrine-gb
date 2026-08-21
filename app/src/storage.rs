mod backend;

use backend::Backend;

pub const THUMB_WIDTH: usize = 80;
pub const THUMB_HEIGHT: usize = 72;

const BATTERY: &str = "battery.sav";
const BATTERY_META: &str = "battery.meta";

fn snapshot_blob(slot: usize) -> String {
    format!("slot{slot}.snap")
}

fn snapshot_meta(slot: usize) -> String {
    format!("slot{slot}.meta")
}

fn snapshot_thumb(slot: usize) -> String {
    format!("slot{slot}.thumb")
}

pub struct SnapshotInfo {
    pub slot: usize,
    pub saved_at: u64,
    pub thumbnail: Vec<u8>,
}

#[derive(Default)]
pub struct SaveStore {
    backend: Backend,
}

impl SaveStore {
    #[cfg(all(test, not(target_arch = "wasm32")))]
    fn at(root: std::path::PathBuf) -> Self {
        Self {
            backend: Backend::at(root),
        }
    }

    pub fn location(&self) -> String {
        self.backend.location()
    }

    pub fn load_battery(&self, rom_key: &str) -> Option<Vec<u8>> {
        self.backend.read(rom_key, BATTERY)
    }

    pub fn store_battery(&self, rom_key: &str, data: &[u8]) -> std::io::Result<()> {
        self.backend.write(rom_key, BATTERY, data)?;
        self.backend
            .write(rom_key, BATTERY_META, &now_unix().to_le_bytes())
    }

    pub fn battery_saved_at(&self, rom_key: &str) -> Option<u64> {
        self.backend
            .read(rom_key, BATTERY_META)
            .and_then(|b| b.try_into().ok())
            .map(u64::from_le_bytes)
    }

    pub fn has_battery(&self, rom_key: &str) -> bool {
        self.backend.exists(rom_key, BATTERY)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn legacy_save_path(rom_path: &std::path::Path) -> std::path::PathBuf {
        rom_path.with_extension("sav")
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn has_legacy_save(rom_path: &std::path::Path) -> bool {
        Self::legacy_save_path(rom_path).is_file()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn import_legacy_save(&self, rom_key: &str, rom_path: &std::path::Path) -> bool {
        let Ok(data) = std::fs::read(Self::legacy_save_path(rom_path)) else {
            return false;
        };
        self.store_battery(rom_key, &data).is_ok()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn import_legacy_save_if_new(&self, rom_key: &str, rom_path: &std::path::Path) -> bool {
        if self.has_battery(rom_key) {
            return false;
        }
        self.import_legacy_save(rom_key, rom_path)
    }

    pub fn store_snapshot(
        &self,
        rom_key: &str,
        slot: usize,
        state: &[u8],
        frame_rgba: &[u8],
    ) -> std::io::Result<()> {
        self.backend.write(rom_key, &snapshot_blob(slot), state)?;
        self.backend.write(
            rom_key,
            &snapshot_thumb(slot),
            &compress(&downscale(frame_rgba)),
        )?;
        self.backend
            .write(rom_key, &snapshot_meta(slot), &now_unix().to_le_bytes())
    }

    pub fn load_snapshot(&self, rom_key: &str, slot: usize) -> Option<Vec<u8>> {
        self.backend.read(rom_key, &snapshot_blob(slot))
    }

    pub fn delete_snapshot(&self, rom_key: &str, slot: usize) -> std::io::Result<()> {
        self.backend.delete(rom_key, &snapshot_meta(slot))?;
        self.backend.delete(rom_key, &snapshot_thumb(slot))?;
        self.backend.delete(rom_key, &snapshot_blob(slot))
    }

    pub fn snapshot_info(&self, rom_key: &str, slot: usize) -> Option<SnapshotInfo> {
        if !self.backend.exists(rom_key, &snapshot_blob(slot)) {
            return None;
        }
        let saved_at = self
            .backend
            .read(rom_key, &snapshot_meta(slot))
            .and_then(|b| b.try_into().ok())
            .map(u64::from_le_bytes)
            .unwrap_or(0);
        let thumbnail = self
            .backend
            .read(rom_key, &snapshot_thumb(slot))
            .and_then(|t| decompress(&t))
            .filter(|t| t.len() == THUMB_WIDTH * THUMB_HEIGHT * 4)
            .unwrap_or_else(|| vec![0; THUMB_WIDTH * THUMB_HEIGHT * 4]);
        Some(SnapshotInfo {
            slot,
            saved_at,
            thumbnail,
        })
    }

    pub fn used_slots(&self, rom_key: &str) -> Vec<usize> {
        let mut slots: Vec<usize> = self
            .backend
            .list(rom_key)
            .iter()
            .filter_map(|name| {
                name.strip_prefix("slot")
                    .and_then(|rest| rest.strip_suffix(".snap"))
                    .and_then(|n| n.parse().ok())
            })
            .collect();
        slots.sort_unstable();
        slots
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

fn compress(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut writer = brotli::CompressorWriter::new(&mut out, 4096, 6, 22);
    if std::io::Write::write_all(&mut writer, data).is_err() {
        drop(writer);
        return data.to_vec();
    }
    drop(writer);
    out
}

fn decompress(data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut reader = brotli::Decompressor::new(data, 4096);
    std::io::Read::read_to_end(&mut reader, &mut out).ok()?;
    Some(out)
}

fn now_unix() -> u64 {
    web_time::SystemTime::now()
        .duration_since(web_time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn downscale(frame: &[u8]) -> Vec<u8> {
    const SRC_WIDTH: usize = THUMB_WIDTH * 2;
    let mut out = vec![0u8; THUMB_WIDTH * THUMB_HEIGHT * 4];
    if frame.len() < SRC_WIDTH * THUMB_HEIGHT * 2 * 4 {
        return out;
    }
    for y in 0..THUMB_HEIGHT {
        for x in 0..THUMB_WIDTH {
            for channel in 0..4 {
                let at = |dy: usize, dx: usize| -> u32 {
                    frame[(((y * 2 + dy) * SRC_WIDTH) + x * 2 + dx) * 4 + channel] as u32
                };
                let sum = at(0, 0) + at(0, 1) + at(1, 0) + at(1, 1);
                out[(y * THUMB_WIDTH + x) * 4 + channel] = (sum / 4) as u8;
            }
        }
    }
    out
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    const KEY: &str = "ABC123";

    fn store(name: &str) -> (SaveStore, std::path::PathBuf) {
        let root =
            std::env::temp_dir().join(format!("citrine-store-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        (SaveStore::at(root.clone()), root)
    }

    fn frame() -> Vec<u8> {
        (0..160 * 144 * 4).map(|i| (i % 256) as u8).collect()
    }

    #[test]
    fn battery_round_trips() {
        let (store, root) = store("battery");
        assert!(store.load_battery(KEY).is_none());
        assert!(!store.has_battery(KEY));

        store.store_battery(KEY, &[1, 2, 3]).expect("store");
        assert!(store.has_battery(KEY));
        assert_eq!(store.load_battery(KEY).as_deref(), Some(&[1, 2, 3][..]));

        store.store_battery(KEY, &[4, 5]).expect("overwrite");
        assert_eq!(store.load_battery(KEY).as_deref(), Some(&[4, 5][..]));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn writes_leave_no_temp_file_behind() {
        let (store, root) = store("atomic");
        store.store_battery(KEY, &[7; 64]).expect("store");
        let leftovers: Vec<_> = std::fs::read_dir(root.join(KEY))
            .expect("dir")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "stray temp files: {leftovers:?}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn battery_records_when_it_was_written() {
        let (store, root) = store("battery-meta");
        assert!(store.battery_saved_at(KEY).is_none());
        store.store_battery(KEY, &[1]).expect("store");
        assert!(store.battery_saved_at(KEY).is_some_and(|t| t > 0));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn explicit_reimport_overwrites_a_stored_save() {
        let (store, root) = store("reimport");
        let rom_dir = root.join("roms");
        std::fs::create_dir_all(&rom_dir).expect("rom dir");
        let rom = rom_dir.join("game.gb");
        std::fs::write(&rom, b"rom").expect("rom");
        std::fs::write(rom_dir.join("game.sav"), b"from-file").expect("sav");

        assert!(SaveStore::has_legacy_save(&rom));
        store.store_battery(KEY, b"stored").expect("stored");

        assert!(!store.import_legacy_save_if_new(KEY, &rom));
        assert_eq!(store.load_battery(KEY).as_deref(), Some(&b"stored"[..]));

        assert!(store.import_legacy_save(KEY, &rom));
        assert_eq!(store.load_battery(KEY).as_deref(), Some(&b"from-file"[..]));
        assert!(rom_dir.join("game.sav").exists(), "original is kept");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn no_legacy_file_means_no_import() {
        let (store, root) = store("no-legacy");
        let rom_dir = root.join("roms");
        std::fs::create_dir_all(&rom_dir).expect("rom dir");
        let rom = rom_dir.join("game.gb");
        std::fs::write(&rom, b"rom").expect("rom");

        assert!(!SaveStore::has_legacy_save(&rom));
        assert!(!store.import_legacy_save_if_new(KEY, &rom));
        assert!(store.load_battery(KEY).is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_save_is_adopted_once_and_original_kept() {
        let (store, root) = store("legacy");
        let rom_dir = root.join("roms");
        std::fs::create_dir_all(&rom_dir).expect("rom dir");
        let rom = rom_dir.join("game.gb");
        std::fs::write(&rom, b"rom").expect("rom");
        std::fs::write(rom_dir.join("game.sav"), b"old-save").expect("sav");

        assert!(
            store.import_legacy_save_if_new(KEY, &rom),
            "first import runs"
        );
        assert_eq!(store.load_battery(KEY).as_deref(), Some(&b"old-save"[..]));
        assert!(rom_dir.join("game.sav").exists(), "original is kept");

        store.store_battery(KEY, b"newer").expect("newer");
        assert!(
            !store.import_legacy_save_if_new(KEY, &rom),
            "does not re-import"
        );
        assert_eq!(store.load_battery(KEY).as_deref(), Some(&b"newer"[..]));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn snapshot_round_trips_with_metadata() {
        let (store, root) = store("snapshot");
        assert!(store.snapshot_info(KEY, 0).is_none());

        store
            .store_snapshot(KEY, 3, b"state-bytes", &frame())
            .expect("store");
        assert_eq!(
            store.load_snapshot(KEY, 3).as_deref(),
            Some(&b"state-bytes"[..])
        );

        let info = store.snapshot_info(KEY, 3).expect("filled");
        assert_eq!(info.slot, 3);
        assert!(info.saved_at > 0, "timestamp recorded");
        assert_eq!(info.thumbnail.len(), THUMB_WIDTH * THUMB_HEIGHT * 4);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn slots_are_unbounded_and_listed_in_order() {
        let (store, root) = store("unbounded");
        for slot in [0, 5, 99, 1000] {
            store
                .store_snapshot(KEY, slot, b"state", &frame())
                .expect("store");
        }
        assert_eq!(store.used_slots(KEY), vec![0, 5, 99, 1000]);
        assert_eq!(store.snapshot_infos(KEY).len(), 4);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn next_free_slot_fills_gaps_before_extending() {
        let (store, root) = store("nextfree");
        assert_eq!(store.next_free_slot(KEY), 0);

        for slot in [0, 1, 2] {
            store
                .store_snapshot(KEY, slot, b"state", &frame())
                .expect("store");
        }
        assert_eq!(store.next_free_slot(KEY), 3);

        store.delete_snapshot(KEY, 1).expect("delete");
        assert_eq!(store.next_free_slot(KEY), 1, "reuses the gap");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn thumbnails_are_stored_compressed() {
        let (store, root) = store("compress");
        store
            .store_snapshot(KEY, 0, b"state", &frame())
            .expect("store");

        let raw = THUMB_WIDTH * THUMB_HEIGHT * 4;
        let on_disk = std::fs::metadata(root.join(KEY).join("slot0.thumb"))
            .expect("thumb")
            .len() as usize;
        assert!(
            on_disk < raw / 2,
            "expected compression, got {on_disk} of {raw} bytes"
        );

        let info = store.snapshot_info(KEY, 0).expect("filled");
        assert_eq!(info.thumbnail.len(), raw);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn snapshots_and_battery_share_one_namespace() {
        let (store, root) = store("namespace");
        store.store_battery(KEY, b"sram").expect("battery");
        store
            .store_snapshot(KEY, 0, b"state", &frame())
            .expect("snapshot");
        assert!(root.join(KEY).join("battery.sav").exists());
        assert!(root.join(KEY).join("slot0.snap").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn thumbnail_averages_each_2x2_block() {
        let mut frame = vec![0u8; 160 * 144 * 4];
        for (i, dx) in [(0usize, 0usize), (1, 4), (160, 8), (161, 12)] {
            frame[i * 4] = dx as u8;
        }
        let thumb = downscale(&frame);
        assert_eq!(thumb.len(), THUMB_WIDTH * THUMB_HEIGHT * 4);
        assert_eq!(thumb[0], 6);
    }
}

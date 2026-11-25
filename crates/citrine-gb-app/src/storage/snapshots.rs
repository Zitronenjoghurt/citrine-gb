use super::{
    SaveStore, SnapshotInfo, compress, decode_meta, decompress_at_most, downscale, encode_meta,
    now_unix, snapshot_blob, snapshot_filed, snapshot_meta, snapshot_name, snapshot_progress,
    snapshot_thumb, thumb_size,
};
use citrine_gb::console::ConsoleKind;
use citrine_gb::persistence::state::state_kind;

pub fn timed_name(name: &str, at: u64, times: &[u64]) -> String {
    format!("{name} from {}", list_stamp(at, times))
}

pub fn list_stamp(at: u64, times: &[u64]) -> String {
    let mut minutes: Vec<u64> = times.iter().map(|t| t / 60).collect();
    minutes.sort_unstable();
    let format = if minutes.windows(2).any(|pair| pair[0] == pair[1]) {
        "%b %-d, %H:%M:%S"
    } else {
        "%b %-d, %H:%M"
    };
    crate::media::local_stamp(at, format)
}

impl SaveStore {
    pub fn store_snapshot(
        &self,
        rom_key: &str,
        slot: usize,
        state: &[u8],
        frame_rgba: &[u8],
        console: ConsoleKind,
    ) -> std::io::Result<()> {
        self.delete(rom_key, &snapshot_filed(slot))?;
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

    pub fn store_snapshot_progress(
        &self,
        rom_key: &str,
        slot: usize,
        state: &[u8],
        progress: Option<&[u8]>,
    ) -> std::io::Result<()> {
        match progress {
            Some(progress) => {
                let mut file = state_mark(state).to_le_bytes().to_vec();
                file.extend_from_slice(progress);
                self.write(rom_key, &snapshot_progress(slot), &file)
            }
            None => self.delete(rom_key, &snapshot_progress(slot)),
        }
    }

    pub fn load_snapshot_progress(
        &self,
        rom_key: &str,
        slot: usize,
        state: &[u8],
    ) -> Option<Vec<u8>> {
        let file = self.backend.read(rom_key, &snapshot_progress(slot))?;
        let (mark, progress) = file.split_at_checked(8)?;
        let mark = u64::from_le_bytes(mark.try_into().ok()?);
        (mark == state_mark(state)).then(|| progress.to_vec())
    }

    pub fn delete_snapshot(&self, rom_key: &str, slot: usize) -> std::io::Result<()> {
        self.delete(rom_key, &snapshot_filed(slot))?;
        self.delete(rom_key, &snapshot_progress(slot))?;
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
        info.filed = self.is_filed_autosave(rom_key, slot);
        Some(info)
    }

    pub fn file_as_autosave(&self, rom_key: &str, slot: usize) -> std::io::Result<()> {
        self.write(rom_key, &snapshot_filed(slot), b"1")
    }

    pub fn is_filed_autosave(&self, rom_key: &str, slot: usize) -> bool {
        self.backend.exists(rom_key, &snapshot_filed(slot))
    }

    pub fn spoken_snapshot_name(&self, rom_key: &str, slot: usize) -> Option<String> {
        let name = self.snapshot_name(rom_key, slot)?;
        if !self.is_filed_autosave(rom_key, slot) {
            return Some(name);
        }
        let times: Vec<(usize, u64)> = self
            .used_slots(rom_key)
            .into_iter()
            .filter(|&other| self.is_filed_autosave(rom_key, other))
            .filter_map(|other| Some((other, self.snapshot_info(rom_key, other)?.saved_at)))
            .collect();
        let Some(&(_, at)) = times.iter().find(|(other, _)| *other == slot) else {
            return Some(name);
        };
        let times: Vec<u64> = times
            .into_iter()
            .map(|(_, at)| at)
            .chain(
                self.autosave_ids(rom_key)
                    .into_iter()
                    .filter_map(|id| self.autosave_saved_at(rom_key, id)),
            )
            .collect();
        Some(timed_name(&name, at, &times))
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
            .and_then(|t| decompress_at_most(&t, width * height * 4))
            .filter(|t| t.len() == width * height * 4)
            .unwrap_or_else(|| vec![0; width * height * 4]);
        Some(SnapshotInfo {
            slot,
            saved_at,
            thumbnail,
            name: None,
            console,
            filed: false,
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

    pub fn move_snapshot(&self, from: &str, slot: usize, to: &str) -> std::io::Result<usize> {
        let moved = self.next_free_slot(to);
        self.copy_files(
            from,
            to,
            &[
                (snapshot_blob(slot), snapshot_blob(moved)),
                (snapshot_thumb(slot), snapshot_thumb(moved)),
                (snapshot_meta(slot), snapshot_meta(moved)),
                (snapshot_name(slot), snapshot_name(moved)),
                (snapshot_progress(slot), snapshot_progress(moved)),
                (snapshot_filed(slot), snapshot_filed(moved)),
            ],
        )?;
        self.delete_snapshot(from, slot)?;
        Ok(moved)
    }

    pub(super) fn copy_files(
        &self,
        from: &str,
        to: &str,
        names: &[(String, String)],
    ) -> std::io::Result<()> {
        for (old, new) in names {
            if let Some(data) = self.backend.read(from, old) {
                self.write(to, new, &data)?;
            }
        }
        Ok(())
    }
}

fn state_mark(state: &[u8]) -> u64 {
    state.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    }) ^ state.len() as u64
}

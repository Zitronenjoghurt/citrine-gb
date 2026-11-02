use super::{
    SaveStore, SnapshotInfo, autosave_blob, autosave_cloud, autosave_meta, autosave_thumb,
    compress, decode_meta, downscale, encode_meta, now_unix,
};
use citrine_gb::gb::GbModel;

impl SaveStore {
    pub fn store_autosave(
        &self,
        rom_key: &str,
        state: &[u8],
        frame_rgba: &[u8],
        model: GbModel,
        keep: usize,
    ) -> std::io::Result<usize> {
        let id = self
            .autosave_ids(rom_key)
            .into_iter()
            .max()
            .map_or(0, |last| last + 1);
        let thumb = compress(&downscale(frame_rgba));
        let written = self.write_autosave(rom_key, id, state, &thumb, model);
        let written = match written {
            Err(_) if self.drop_oldest_autosave(rom_key, id)? => {
                self.write_autosave(rom_key, id, state, &thumb, model)
            }
            other => other,
        };
        if let Err(err) = written {
            self.delete_autosave(rom_key, id)?;
            return Err(err);
        }
        self.prune_autosaves(rom_key, keep)?;
        Ok(id)
    }

    pub(super) fn write_autosave(
        &self,
        rom_key: &str,
        id: usize,
        state: &[u8],
        thumb: &[u8],
        model: GbModel,
    ) -> std::io::Result<()> {
        self.write(rom_key, &autosave_blob(id), state)?;
        self.write(rom_key, &autosave_thumb(id), thumb)?;
        self.write(rom_key, &autosave_meta(id), &encode_meta(now_unix(), model))
    }

    pub(super) fn drop_oldest_autosave(
        &self,
        rom_key: &str,
        writing: usize,
    ) -> std::io::Result<bool> {
        match self
            .autosave_ids(rom_key)
            .into_iter()
            .find(|id| *id != writing)
        {
            Some(oldest) => self.delete_autosave(rom_key, oldest).map(|()| true),
            None => Ok(false),
        }
    }

    pub fn prune_autosaves(&self, rom_key: &str, keep: usize) -> std::io::Result<()> {
        let ids = self.autosave_ids(rom_key);
        let extra = ids.len().saturating_sub(keep.max(1));
        for id in &ids[..extra] {
            self.delete_autosave(rom_key, *id)?;
        }
        Ok(())
    }

    pub fn autosave_ids(&self, rom_key: &str) -> Vec<usize> {
        let mut ids: Vec<(u64, usize)> = self
            .numbered(rom_key, "auto")
            .into_iter()
            .map(|id| {
                let saved_at = self
                    .backend
                    .read(rom_key, &autosave_meta(id))
                    .and_then(|meta| decode_meta(&meta).0)
                    .unwrap_or(0);
                (saved_at, id)
            })
            .collect();
        ids.sort_unstable();
        ids.into_iter().map(|(_, id)| id).collect()
    }

    pub fn autosave_infos(&self, rom_key: &str) -> Vec<SnapshotInfo> {
        self.autosave_ids(rom_key)
            .into_iter()
            .rev()
            .filter_map(|id| {
                self.read_info(
                    rom_key,
                    id,
                    &autosave_blob(id),
                    &autosave_meta(id),
                    &autosave_thumb(id),
                )
            })
            .collect()
    }

    pub fn load_autosave(&self, rom_key: &str, id: usize) -> Option<Vec<u8>> {
        self.backend.read(rom_key, &autosave_blob(id))
    }

    pub fn delete_autosave(&self, rom_key: &str, id: usize) -> std::io::Result<()> {
        self.delete(rom_key, &autosave_cloud(id))?;
        self.delete(rom_key, &autosave_meta(id))?;
        self.delete(rom_key, &autosave_thumb(id))?;
        self.delete(rom_key, &autosave_blob(id))
    }
}

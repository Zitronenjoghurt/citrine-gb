use super::{
    MediaId, MediaInfo, MediaItem, MediaKind, NewMedia, decode_png, encode_thumbnail,
    game_from_file_name, png_size, sort_newest_first, unique_file_name,
};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

const LIBRARY: &str = ".library";
const TEMP: &str = "temp";
const INFO_SUFFIX: &str = ".json";
const THUMB_SUFFIX: &str = ".thumb.png";
const MAX_DECODE_BYTES: u64 = 16 * 1024 * 1024;

pub struct MediaStore {
    root: PathBuf,
    items: Vec<MediaItem>,
}

impl Default for MediaStore {
    fn default() -> Self {
        let store = Self::at(media_root());
        store.clear_temp();
        store
    }
}

fn media_root() -> PathBuf {
    crate::storage::data_dir().join("media")
}

pub fn recording_temp_path() -> io::Result<PathBuf> {
    temp_path_in(&media_root())
}

fn temp_path_in(root: &Path) -> io::Result<PathBuf> {
    let dir = root.join(LIBRARY).join(TEMP);
    std::fs::create_dir_all(&dir)?;
    let stamp = web_time::SystemTime::now()
        .duration_since(web_time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    Ok(dir.join(format!("recording-{stamp}.mp4.part")))
}

impl MediaStore {
    pub fn at(root: PathBuf) -> Self {
        let mut store = Self {
            root,
            items: Vec::new(),
        };
        store.refresh();
        store
    }

    pub fn items(&self) -> &[MediaItem] {
        &self.items
    }

    pub fn path(&self, id: &MediaId) -> PathBuf {
        self.root.join(id.as_str())
    }

    pub fn temp_path(&self) -> io::Result<PathBuf> {
        temp_path_in(&self.root)
    }

    fn folder(&self, kind: MediaKind) -> PathBuf {
        self.root.join(kind.folder())
    }

    fn sidecar_folder(&self, kind: MediaKind) -> PathBuf {
        self.root.join(LIBRARY).join(kind.folder())
    }

    fn info_path(&self, kind: MediaKind, file_name: &str) -> PathBuf {
        self.sidecar_folder(kind)
            .join(format!("{file_name}{INFO_SUFFIX}"))
    }

    fn thumb_path(&self, kind: MediaKind, file_name: &str) -> PathBuf {
        self.sidecar_folder(kind)
            .join(format!("{file_name}{THUMB_SUFFIX}"))
    }

    fn clear_temp(&self) {
        let _ = std::fs::remove_dir_all(self.root.join(LIBRARY).join(TEMP));
    }

    pub fn add_screenshot(&mut self, new: NewMedia, png: &[u8]) -> io::Result<MediaItem> {
        let temp = self.temp_path()?;
        std::fs::write(&temp, png)?;
        self.adopt(&temp, new)
    }

    pub fn add_recording(&mut self, temp: &Path, new: NewMedia) -> io::Result<MediaItem> {
        self.adopt(temp, new)
    }

    fn adopt(&mut self, temp: &Path, new: NewMedia) -> io::Result<MediaItem> {
        let kind = new.kind;
        let folder = self.folder(kind);
        std::fs::create_dir_all(&folder)?;
        std::fs::create_dir_all(self.sidecar_folder(kind))?;
        let stem = super::file_stem(&new.game, new.created_at);
        let file_name =
            unique_file_name(&stem, kind.extension(), |name| folder.join(name).exists());
        let target = folder.join(&file_name);
        if std::fs::rename(temp, &target).is_err() {
            std::fs::copy(temp, &target)?;
            let _ = std::fs::remove_file(temp);
        }
        let bytes = std::fs::metadata(&target)?.len();
        let info = new.info(bytes);
        let thumb_png = encode_thumbnail(&new.frame);
        if let Some(png) = &thumb_png {
            std::fs::write(self.thumb_path(kind, &file_name), png)?;
        }
        let json = serde_json::to_vec_pretty(&info).map_err(io::Error::other)?;
        std::fs::write(self.info_path(kind, &file_name), json)?;
        let item = MediaItem {
            id: MediaId::new(kind, &file_name),
            info,
            thumbnail: thumb_png.as_deref().and_then(decode_png),
        };
        self.items.retain(|i| i.id != item.id);
        self.items.push(item.clone());
        sort_newest_first(&mut self.items);
        Ok(item)
    }

    pub fn delete(&mut self, id: &MediaId) -> io::Result<()> {
        let kind = id.kind();
        remove_if_present(&self.path(id))?;
        if let Some(kind) = kind {
            remove_if_present(&self.info_path(kind, id.file_name()))?;
            remove_if_present(&self.thumb_path(kind, id.file_name()))?;
        }
        self.items.retain(|i| &i.id != id);
        Ok(())
    }

    pub fn refresh(&mut self) {
        let mut known: HashMap<MediaId, MediaItem> = std::mem::take(&mut self.items)
            .into_iter()
            .map(|item| (item.id.clone(), item))
            .collect();
        let mut items = Vec::new();
        for kind in MediaKind::ALL {
            let files = self.media_files(kind);
            for (file_name, metadata) in &files {
                let id = MediaId::new(kind, file_name);
                let bytes = metadata.len();
                let item = match known.remove(&id) {
                    Some(item) if item.info.bytes == bytes => item,
                    _ => self.read_item(kind, file_name, metadata),
                };
                items.push(item);
            }
            self.prune_sidecars(kind, &files);
        }
        sort_newest_first(&mut items);
        self.items = items;
    }

    fn media_files(&self, kind: MediaKind) -> Vec<(String, std::fs::Metadata)> {
        let Ok(entries) = std::fs::read_dir(self.folder(kind)) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                let metadata = entry.metadata().ok()?;
                let matches = Path::new(&name)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case(kind.extension()));
                (metadata.is_file() && matches && !name.starts_with('.'))
                    .then_some((name, metadata))
            })
            .collect()
    }

    fn read_item(
        &self,
        kind: MediaKind,
        file_name: &str,
        metadata: &std::fs::Metadata,
    ) -> MediaItem {
        let path = self.folder(kind).join(file_name);
        let stored = std::fs::read(self.info_path(kind, file_name))
            .ok()
            .and_then(|raw| serde_json::from_slice::<MediaInfo>(&raw).ok())
            .filter(|info| info.kind == kind);
        let file_png = (kind == MediaKind::Screenshot && metadata.len() <= MAX_DECODE_BYTES)
            .then(|| std::fs::read(&path).ok())
            .flatten();
        let info = match stored {
            Some(mut info) => {
                info.bytes = metadata.len();
                info
            }
            None => {
                let (width, height) = file_png.as_deref().and_then(png_size).unwrap_or((0, 0));
                MediaInfo {
                    kind,
                    game: game_from_file_name(file_name),
                    rom_key: None,
                    model: None,
                    created_at: modified_unix(metadata),
                    bytes: metadata.len(),
                    width,
                    height,
                    length_ms: None,
                }
            }
        };
        let thumbnail = std::fs::read(self.thumb_path(kind, file_name))
            .ok()
            .and_then(|png| decode_png(&png))
            .or_else(|| {
                file_png
                    .as_deref()
                    .and_then(decode_png)
                    .and_then(|frame| super::thumbnail_of(&frame))
            });
        MediaItem {
            id: MediaId::new(kind, file_name),
            info,
            thumbnail,
        }
    }

    fn prune_sidecars(&self, kind: MediaKind, files: &[(String, std::fs::Metadata)]) {
        let Ok(entries) = std::fs::read_dir(self.sidecar_folder(kind)) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let owner = name
                .strip_suffix(THUMB_SUFFIX)
                .or_else(|| name.strip_suffix(INFO_SUFFIX));
            if let Some(owner) = owner
                && !files.iter().any(|(file, _)| file == owner)
            {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

fn modified_unix(metadata: &std::fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::Frame;
    use citrine_gb::gb::GbModel;

    fn store(name: &str) -> (MediaStore, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("citrine-media-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        (MediaStore::at(root.clone()), root)
    }

    fn new_media(kind: MediaKind, created_at: u64) -> NewMedia {
        NewMedia {
            kind,
            game: "TETRIS".to_string(),
            rom_key: Some("abc".to_string()),
            model: GbModel::Dmg,
            created_at,
            width: 160,
            height: 144,
            length_ms: (kind == MediaKind::Recording).then_some(4200),
            frame: Frame {
                width: 160,
                height: 144,
                rgba: vec![80; 160 * 144 * 4],
            },
        }
    }

    fn png() -> Vec<u8> {
        crate::utils::screenshot::encode_png(160, 144, &vec![40; 160 * 144 * 4]).unwrap()
    }

    #[test]
    fn a_screenshot_is_added_and_listed_with_its_facts() {
        let (mut store, root) = store("add");
        let item = store
            .add_screenshot(new_media(MediaKind::Screenshot, 1_700_000_000), &png())
            .unwrap();
        assert!(item.id.file_name().starts_with("TETRIS "));
        assert!(item.id.file_name().ends_with(".png"));
        assert!(root.join("screenshots").join(item.id.file_name()).is_file());
        assert_eq!(item.info.bytes, png().len() as u64);
        assert_eq!(item.info.model, Some(GbModel::Dmg));
        let thumb = item.thumbnail.as_ref().expect("thumbnail");
        assert_eq!((thumb.width, thumb.height), (160, 144));

        let reopened = MediaStore::at(root.clone());
        assert_eq!(reopened.items().len(), 1);
        assert_eq!(reopened.items()[0].info, item.info);
        assert!(reopened.items()[0].thumbnail.is_some());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_list_is_newest_first_across_kinds() {
        let (mut store, root) = store("order");
        store
            .add_screenshot(new_media(MediaKind::Screenshot, 100), &png())
            .unwrap();
        let temp = store.temp_path().unwrap();
        std::fs::write(&temp, b"mp4").unwrap();
        store
            .add_recording(&temp, new_media(MediaKind::Recording, 300))
            .unwrap();
        store
            .add_screenshot(new_media(MediaKind::Screenshot, 200), &png())
            .unwrap();
        let order: Vec<u64> = MediaStore::at(root.clone())
            .items()
            .iter()
            .map(|i| i.info.created_at)
            .collect();
        assert_eq!(order, vec![300, 200, 100]);
        assert!(!temp.exists(), "the temp file moved into place");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_recording_moves_into_the_recordings_folder() {
        let (mut store, root) = store("recording");
        let temp = store.temp_path().unwrap();
        assert!(temp.starts_with(root.join(LIBRARY)));
        std::fs::write(&temp, vec![7; 1234]).unwrap();
        let item = store
            .add_recording(&temp, new_media(MediaKind::Recording, 50))
            .unwrap();
        assert_eq!(item.info.kind, MediaKind::Recording);
        assert_eq!(item.info.bytes, 1234);
        assert_eq!(item.info.length_ms, Some(4200));
        assert!(root.join("recordings").join(item.id.file_name()).is_file());
        assert!(item.id.file_name().ends_with(".mp4"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn two_in_the_same_second_both_keep_their_file() {
        let (mut store, root) = store("same-second");
        let a = store
            .add_screenshot(new_media(MediaKind::Screenshot, 10), &png())
            .unwrap();
        let b = store
            .add_screenshot(new_media(MediaKind::Screenshot, 10), &png())
            .unwrap();
        assert_ne!(a.id, b.id);
        assert!(b.id.file_name().ends_with(" (2).png"));
        assert_eq!(store.items().len(), 2);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn delete_removes_the_file_and_its_sidecars() {
        let (mut store, root) = store("delete");
        let item = store
            .add_screenshot(new_media(MediaKind::Screenshot, 10), &png())
            .unwrap();
        store.delete(&item.id).unwrap();
        assert!(store.items().is_empty());
        assert!(!store.path(&item.id).exists());
        let leftovers = std::fs::read_dir(root.join(LIBRARY).join("screenshots"))
            .unwrap()
            .count();
        assert_eq!(leftovers, 0);
        store.delete(&item.id).unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_file_deleted_by_hand_leaves_the_list() {
        let (mut store, root) = store("hand-deleted");
        let item = store
            .add_screenshot(new_media(MediaKind::Screenshot, 10), &png())
            .unwrap();
        std::fs::remove_file(store.path(&item.id)).unwrap();
        store.refresh();
        assert!(store.items().is_empty());
        let leftovers = std::fs::read_dir(root.join(LIBRARY).join("screenshots"))
            .unwrap()
            .count();
        assert_eq!(leftovers, 0, "orphan sidecars are cleared");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_file_added_by_hand_joins_the_list() {
        let (mut store, root) = store("hand-added");
        std::fs::create_dir_all(root.join("screenshots")).unwrap();
        std::fs::create_dir_all(root.join("recordings")).unwrap();
        std::fs::write(
            root.join("screenshots")
                .join("ZELDA 2026-01-02 03-04-05.png"),
            png(),
        )
        .unwrap();
        std::fs::write(root.join("screenshots").join("notes.txt"), b"hi").unwrap();
        std::fs::write(root.join("screenshots").join(".hidden.png"), png()).unwrap();
        std::fs::write(root.join("recordings").join("clip.MP4"), b"mp4").unwrap();
        store.refresh();
        assert_eq!(store.items().len(), 2);
        let shot = store
            .items()
            .iter()
            .find(|i| i.info.kind == MediaKind::Screenshot)
            .unwrap();
        assert_eq!(shot.info.game, "ZELDA");
        assert_eq!((shot.info.width, shot.info.height), (160, 144));
        assert!(shot.info.created_at > 0);
        assert!(
            shot.thumbnail.is_some(),
            "a hand-added picture is its own thumbnail"
        );
        let clip = store
            .items()
            .iter()
            .find(|i| i.info.kind == MediaKind::Recording)
            .unwrap();
        assert_eq!(clip.info.game, "clip");
        assert_eq!(clip.info.bytes, 3);
        assert!(clip.thumbnail.is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn leftover_temp_files_are_cleared_on_start() {
        let (store, root) = store("temp");
        let temp = store.temp_path().unwrap();
        std::fs::write(&temp, b"half").unwrap();
        store.clear_temp();
        assert!(!temp.exists());
        let _ = std::fs::remove_dir_all(root);
    }
}

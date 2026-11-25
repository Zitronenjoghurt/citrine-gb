mod cover;
mod pack;

use crate::config::Limits;
use crate::db::{ArchiveRow, Db};
use crate::storage::Storage;
use citrine_gb_gameinfo::covers::names::System;
use citrine_gb_gameinfo::header::{self, RomKind};
use citrine_gb_net::{ARCHIVE_PREFIX_BYTES, ArchiveGame, ArchiveId, ArchiveKey, archive_parts};
use ring::hmac;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const ID_INFO: &[u8] = b"citrine archive id";
const SEAL_INFO: &[u8] = b"citrine archive seal";
const KEY_BYTES: usize = 32;

pub use citrine_gb_net::ArchiveConsole as Console;
use cover::Covers;

#[derive(Serialize, Deserialize)]
struct Meta {
    name: String,
    console: Console,
    size: u64,
    sha256: [u8; 32],
    sha1: [u8; 20],
    crc32: u32,
    prefix: [u8; ARCHIVE_PREFIX_BYTES],
    #[serde(default)]
    serial: Option<[u8; 4]>,
    #[serde(default)]
    colour: bool,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub id: ArchiveId,
    pub stored: u64,
    pub added_at: u64,
    pub game: Option<ArchiveGame>,
}

pub struct Opened {
    pub key: ArchiveKey,
    pub prefix: [u8; ARCHIVE_PREFIX_BYTES],
    pub parts: u32,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Added {
    pub id: ArchiveId,
    pub new: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum AddError {
    #[error("the Game Archive is off")]
    Off,
    #[error("the file is empty")]
    Empty,
    #[error("the file is over the {0} MB limit")]
    TooLarge(u64),
    #[error("the Game Archive holds {0} games, its limit")]
    TooMany(u64),
    #[error("the Game Archive is full")]
    Full,
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("database: {0}")]
    Db(#[from] sqlx::Error),
}

struct Keys {
    id: hmac::Key,
    seal: ArchiveKey,
}

impl Keys {
    fn from_master(master: &[u8; KEY_BYTES]) -> Self {
        let master = hmac::Key::new(hmac::HMAC_SHA256, master);
        let id = hmac::sign(&master, ID_INFO);
        let mut seal = [0; KEY_BYTES];
        seal.copy_from_slice(hmac::sign(&master, SEAL_INFO).as_ref());
        Self {
            id: hmac::Key::new(hmac::HMAC_SHA256, id.as_ref()),
            seal: ArchiveKey(seal),
        }
    }
}

pub struct Archive {
    keys: Option<Keys>,
    dir: PathBuf,
    total_bytes: u64,
    file_bytes: u64,
    max_games: u64,
    adding: tokio::sync::Mutex<()>,
    uploading: Arc<tokio::sync::Mutex<()>>,
    downloading: Mutex<HashSet<i64>>,
    covers: Option<Covers>,
}

pub struct Downloading {
    archive: Arc<Archive>,
    account: i64,
}

impl Drop for Downloading {
    fn drop(&mut self) {
        self.archive.downloading().remove(&self.account);
    }
}

fn load_key(path: &Path, data_dir: &Path) -> io::Result<[u8; KEY_BYTES]> {
    if path.starts_with(data_dir) {
        log::warn!(
            "CITRINE_ARCHIVE_KEY_FILE is inside the data folder, so a copy of the data folder can open the Game Archive. Keep the key file somewhere else"
        );
    }
    match std::fs::read(path) {
        Ok(bytes) => bytes.try_into().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "the Game Archive key file {} must hold exactly {KEY_BYTES} bytes",
                    path.display()
                ),
            )
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
            let key = ArchiveKey::random().0;
            let mut options = std::fs::File::options();
            options.write(true).create_new(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
            let mut file = options.open(path)?;
            io::Write::write_all(&mut file, &key)?;
            file.sync_all()?;
            log::info!(
                "made a new Game Archive key in {}. Back it up: without it the archive can't be read",
                path.display()
            );
            Ok(key)
        }
        Err(e) => Err(e),
    }
}

fn name_of(console: Console, hashes: &pack::Hashes, file_name: &str) -> String {
    let kind = match console {
        Console::GameBoy | Console::GameBoyColor => RomKind::GameBoy,
        Console::GameBoyAdvance => RomKind::Advance,
        Console::Ds => RomKind::Ds,
        Console::Other => {
            return Path::new(file_name)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(file_name)
                .to_string();
        }
    };
    header::rom_name(kind, &hashes.sha1, &hashes.head, file_name)
}

impl Archive {
    pub async fn open(
        key_file: Option<&Path>,
        data_dir: &Path,
        limits: &Limits,
        covers: bool,
    ) -> io::Result<Self> {
        let dir = data_dir.join("archive");
        tokio::fs::create_dir_all(&dir).await?;
        let mut entries = tokio::fs::read_dir(&dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            if entry
                .path()
                .extension()
                .is_some_and(|e| e == "part" || e == "upload")
            {
                let _ = tokio::fs::remove_file(entry.path()).await;
            }
        }
        let keys = match key_file {
            Some(path) => Some(Keys::from_master(&load_key(path, data_dir)?)),
            None => None,
        };
        Ok(Self {
            keys,
            dir,
            total_bytes: limits.archive_total_bytes,
            file_bytes: limits.archive_file_bytes,
            max_games: limits.archive_games,
            adding: tokio::sync::Mutex::new(()),
            uploading: Arc::default(),
            downloading: Mutex::new(HashSet::new()),
            covers: match covers {
                true => Some(Covers::new(&data_dir.join("cover-names")).map_err(io::Error::other)?),
                false => None,
            },
        })
    }

    pub fn on(&self) -> bool {
        self.keys.is_some()
    }

    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    pub fn file_bytes(&self) -> u64 {
        self.file_bytes
    }

    pub fn max_games(&self) -> u64 {
        self.max_games
    }

    pub fn upload_slot(&self) -> Option<tokio::sync::OwnedMutexGuard<()>> {
        self.uploading.clone().try_lock_owned().ok()
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path(&self, id: ArchiveId) -> PathBuf {
        self.dir.join(id.to_string())
    }

    fn downloading(&self) -> std::sync::MutexGuard<'_, HashSet<i64>> {
        self.downloading.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn start_download(self: &Arc<Self>, account: i64) -> Option<Downloading> {
        self.downloading().insert(account).then(|| Downloading {
            archive: self.clone(),
            account,
        })
    }

    fn id_of(keys: &Keys, sha256: &[u8; 32]) -> ArchiveId {
        let mut id = [0; 16];
        id.copy_from_slice(&hmac::sign(&keys.id, sha256).as_ref()[..16]);
        ArchiveId(id)
    }

    pub async fn add_file(
        &self,
        db: &Db,
        storage: &Storage,
        src: &Path,
        file_name: &str,
        now: u64,
        progress: impl FnMut(u64) -> bool + Send + 'static,
    ) -> Result<Added, AddError> {
        let keys = self.keys.as_ref().ok_or(AddError::Off)?;
        let _one = self.adding.lock().await;
        let size = tokio::fs::metadata(src).await?.len();
        if size == 0 {
            return Err(AddError::Empty);
        }
        if size > self.file_bytes {
            return Err(AddError::TooLarge(self.file_bytes >> 20));
        }
        let path = src.to_owned();
        let hashes = tokio::task::spawn_blocking(move || pack::hash(&path))
            .await
            .map_err(io::Error::other)??;
        let id = Self::id_of(keys, &hashes.sha256);
        if db.archive_game(id).await?.is_some() {
            return Ok(Added { id, new: false });
        }
        let totals = db.archive_totals().await?;
        if totals.games >= self.max_games {
            return Err(AddError::TooMany(self.max_games));
        }
        if totals.stored >= self.total_bytes || storage.disk_room().is_some_and(|room| room < size)
        {
            return Err(AddError::Full);
        }
        let game_key = ArchiveKey::random();
        let mut prefix = [0; ARCHIVE_PREFIX_BYTES];
        getrandom::getrandom(&mut prefix).expect("no system random source");
        let part = self.dir.join(format!("{id}.part"));
        let (src, dst, key) = (src.to_owned(), part.clone(), game_key.clone());
        let packed =
            tokio::task::spawn_blocking(move || pack::pack(&src, &dst, &key, &prefix, progress))
                .await
                .map_err(io::Error::other)
                .and_then(|r| r);
        let stored = match packed {
            Ok(stored) => stored,
            Err(e) => {
                let _ = tokio::fs::remove_file(&part).await;
                return Err(e.into());
            }
        };
        if totals.stored.saturating_add(stored) > self.total_bytes {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(AddError::Full);
        }
        let console = Console::of(file_name);
        let meta = Meta {
            name: name_of(console, &hashes, file_name),
            console,
            size: hashes.size,
            sha256: hashes.sha256,
            sha1: hashes.sha1,
            crc32: hashes.crc32,
            prefix,
            serial: (console == Console::Ds)
                .then(|| header::ds_game_code(&hashes.head))
                .flatten(),
            colour: hashes.head.get(0x143).is_some_and(|flag| flag & 0x80 != 0),
        };
        let row = ArchiveRow {
            id,
            sealed: keys
                .seal
                .seal(&serde_json::to_vec(&meta).map_err(io::Error::other)?),
            wrapped_key: keys.seal.seal(&game_key.0),
            stored,
            added_at: now,
        };
        let path = self.path(id);
        if let Err(e) = tokio::fs::rename(&part, &path).await {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(e.into());
        }
        match db.archive_add(&row).await {
            Ok(new) => Ok(Added { id, new }),
            Err(e) => {
                let _ = tokio::fs::remove_file(&path).await;
                Err(e.into())
            }
        }
    }

    pub async fn delete(&self, db: &Db, id: ArchiveId) -> sqlx::Result<bool> {
        let deleted = db.archive_delete(id).await?;
        for path in [self.path(id), self.cover_path(id)] {
            match tokio::fs::remove_file(path).await {
                Err(e) if e.kind() != io::ErrorKind::NotFound => {
                    log::error!("couldn't remove a Game Archive file: {e}");
                }
                _ => {}
            }
        }
        Ok(deleted)
    }

    fn cover_path(&self, id: ArchiveId) -> PathBuf {
        self.dir.join(format!("{id}.cover"))
    }

    pub fn covers_on(&self) -> bool {
        self.covers.is_some()
    }

    pub fn has_cover(&self, id: ArchiveId) -> bool {
        self.cover_path(id).is_file()
    }

    pub async fn put_cover(&self, id: ArchiveId, png: &[u8]) -> io::Result<()> {
        let keys = self
            .keys
            .as_ref()
            .ok_or_else(|| io::Error::other("the Game Archive is off"))?;
        let path = self.cover_path(id);
        let part = self.dir.join(format!("{id}.cover.part"));
        tokio::fs::write(&part, keys.seal.seal(png)).await?;
        tokio::fs::rename(&part, &path).await
    }

    pub async fn cover(&self, id: ArchiveId) -> Option<Vec<u8>> {
        let keys = self.keys.as_ref()?;
        let sealed = tokio::fs::read(self.cover_path(id)).await.ok()?;
        keys.seal.open(&sealed).ok()
    }

    pub async fn find_cover(&self, db: &Db, id: ArchiveId, file_name: &str) -> bool {
        let (Some(covers), Some(keys)) = (self.covers.as_ref(), self.keys.as_ref()) else {
            return false;
        };
        let Ok(Some(row)) = db.archive_game(id).await else {
            return false;
        };
        let Some(meta) = Self::meta(keys, &row) else {
            return false;
        };
        let systems: &[System] = match meta.console {
            Console::GameBoy | Console::GameBoyColor if meta.colour => &[System::Gbc, System::Gb],
            Console::GameBoy | Console::GameBoyColor => &[System::Gb, System::Gbc],
            Console::GameBoyAdvance => &[System::Gba],
            Console::Ds => &[System::Nds],
            Console::Other => return false,
        };
        let find = cover::Find {
            systems,
            sha1: meta.sha1,
            serial: meta.serial,
            file_name,
        };
        let Ok(Some(png)) = tokio::time::timeout(cover::FIND_TIME, covers.find(find)).await else {
            return false;
        };
        match self.put_cover(id, &png).await {
            Ok(()) => true,
            Err(e) => {
                log::warn!("couldn't keep a Game Archive cover: {e}");
                false
            }
        }
    }

    pub async fn name(&self, db: &Db, id: ArchiveId) -> Option<String> {
        let keys = self.keys.as_ref()?;
        let row = db.archive_game(id).await.ok()??;
        Some(Self::meta(keys, &row)?.name)
    }

    fn meta(keys: &Keys, row: &ArchiveRow) -> Option<Meta> {
        let plain = keys.seal.open(&row.sealed).ok()?;
        serde_json::from_slice(&plain).ok()
    }

    fn entry(&self, row: ArchiveRow) -> Entry {
        let game = self.keys.as_ref().and_then(|keys| {
            let meta = Self::meta(keys, &row)?;
            Some(ArchiveGame {
                id: row.id,
                name: meta.name,
                console: meta.console,
                size: meta.size,
                download: row.stored,
                sha256: meta.sha256,
                sha1: meta.sha1,
                crc32: meta.crc32,
            })
        });
        Entry {
            id: row.id,
            stored: row.stored,
            added_at: row.added_at,
            game,
        }
    }

    pub async fn entries(&self, db: &Db) -> sqlx::Result<Vec<Entry>> {
        Ok(db
            .archive_games()
            .await?
            .into_iter()
            .map(|row| self.entry(row))
            .collect())
    }

    pub async fn games_for(&self, db: &Db, account: i64) -> sqlx::Result<Vec<ArchiveGame>> {
        if !self.on() {
            return Ok(Vec::new());
        }
        let mut games = Vec::new();
        for row in db.archive_games_for(account).await? {
            match self.entry(row).game {
                Some(game) => games.push(game),
                None => log::error!("a Game Archive game doesn't open with this key"),
            }
        }
        Ok(games)
    }

    pub async fn open_game(&self, db: &Db, id: ArchiveId) -> sqlx::Result<Option<Opened>> {
        let Some(keys) = self.keys.as_ref() else {
            return Ok(None);
        };
        let Some(row) = db.archive_game(id).await? else {
            return Ok(None);
        };
        let Some(meta) = Self::meta(keys, &row) else {
            return Ok(None);
        };
        let Some(key) = keys
            .seal
            .open(&row.wrapped_key)
            .ok()
            .and_then(|k| <[u8; KEY_BYTES]>::try_from(k).ok())
        else {
            return Ok(None);
        };
        Ok(Some(Opened {
            key: ArchiveKey(key),
            prefix: meta.prefix,
            parts: archive_parts(row.stored),
            path: self.path(id),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_depend_on_the_key_and_the_hash() {
        let a = Keys::from_master(&[1; KEY_BYTES]);
        let b = Keys::from_master(&[2; KEY_BYTES]);
        let sha = [9; 32];
        assert_eq!(Archive::id_of(&a, &sha), Archive::id_of(&a, &sha));
        assert_ne!(Archive::id_of(&a, &sha), Archive::id_of(&b, &sha));
        assert_ne!(Archive::id_of(&a, &sha), Archive::id_of(&a, &[8; 32]));
        assert_ne!(&Archive::id_of(&a, &sha).0[..], &sha[..16]);
    }

    #[tokio::test]
    async fn an_account_downloads_one_game_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let limits = Limits::from_vars(|_| None).unwrap();
        let archive = Arc::new(
            Archive::open(None, dir.path(), &limits, false)
                .await
                .unwrap(),
        );
        let first = archive.start_download(1).unwrap();
        assert!(archive.start_download(1).is_none());
        assert!(archive.start_download(2).is_some());
        drop(first);
        assert!(archive.start_download(1).is_some());
    }

    #[test]
    fn a_missing_key_file_is_made_once_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keys").join("archive.key");
        let first = load_key(&path, &dir.path().join("data")).unwrap();
        let again = load_key(&path, &dir.path().join("data")).unwrap();
        assert_eq!(first, again);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn a_key_file_of_the_wrong_size_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("archive.key");
        std::fs::write(&path, b"short").unwrap();
        assert!(load_key(&path, &dir.path().join("data")).is_err());
    }
}

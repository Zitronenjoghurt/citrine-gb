use crate::app::ui_state::recent::copies::RomCopies;
#[cfg(not(target_arch = "wasm32"))]
use crate::app::ui_state::recent::{RecentRom, RecentRoms};
use crate::app::ui_state::settings::Settings;
use crate::media::{MediaId, MediaInfo, MediaStore};
use crate::online::Online;
use crate::online::chat::history::{decode_history, encode_history, merge_history};
use crate::progress::Counter;
use crate::stats::{Book, GameRecord, Stats};
use crate::storage::SaveStore;
use crate::storage::kinds::{Folder, Kind as Store, Section, Usage, other_data};
use crate::storage::salvage::salvage;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{self, Read, Write};

pub const FILE_EXTENSION: &str = "zip";
const MANIFEST: &str = "citrine-data.json";
const FORMAT: u64 = 1;
const MAX_FILE_BYTES: u64 = 1 << 30;
const MAX_ENTRY_BYTES: u64 = 64 << 20;
const MAX_MANIFEST_BYTES: u64 = 64 << 10;
pub const STEP_BUDGET: std::time::Duration = std::time::Duration::from_millis(12);

const SAVES: &str = "saves/";
const MEDIA: &str = "media/";
const INFO_SUFFIX: &str = ".json";
const THUMB_SUFFIX: &str = ".thumb.png";
const CHATS: &str = "chats/";
const SETTINGS: &str = "settings.json";
const GAME_NAMES: &str = "game-names.json";
const STATS: &str = "stats.json";
#[cfg(not(target_arch = "wasm32"))]
const RECENT: &str = "recent-games.json";

const KEPT_SETTINGS: &[&str] = &[
    "online",
    "synced",
    "current_tab",
    "game_names",
    "achievements",
    "ds_online",
    "discord",
    "check_updates",
];

pub type Files = BTreeMap<String, Vec<u8>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kind {
    pub name: &'static str,
    pub nouns: (&'static str, &'static str),
    pub store: Store,
    pub system_file: Option<crate::app::system_files::SystemFile>,
}

impl Kind {
    pub fn of(kind: Store) -> Self {
        let nouns = match kind {
            Store::Saves => ("save", "saves"),
            Store::States => ("save state", "save states"),
            Store::Screenshots => ("screenshot", "screenshots"),
            Store::Recordings => ("recording", "recordings"),
            Store::Settings => ("setting", "settings"),
            Store::GameNames => ("game name", "game names"),
            Store::RecentGames => ("recent game", "recent games"),
            Store::SystemFiles => ("BIOS or boot ROM file", "BIOS and boot ROM files"),
            Store::DsWifi => ("DS wifi connection", "DS wifi connections"),
            Store::Stats => ("game's play time", "games' play time"),
            Store::Chats => ("chat message", "chat messages"),
            Store::Pokewalker => ("Pokéwalker save", "Pokéwalker saves"),
            Store::GameCopies => ("game copy", "game copies"),
            _ => ("file", "files"),
        };
        Self {
            name: kind.name(),
            nouns,
            store: kind,
            system_file: None,
        }
    }

    pub fn noun(&self, n: usize) -> String {
        counted(n, self.nouns)
    }
}

fn counted(n: usize, (one, many): (&str, &str)) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub count: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Change {
    pub added: usize,
    pub replaced: usize,
}

pub trait Part {
    fn kind(&self) -> Store;
    fn file_row(&self) -> Option<Kind> {
        None
    }
    fn tally(&mut self) -> Tally;
    fn export(&mut self, out: &mut Files);
    fn plan(&mut self, files: &Files) -> Change;
    fn import(&mut self, files: &Files) -> io::Result<()>;
    fn delete_all(&mut self) -> io::Result<()>;
}

pub struct Parts<'a> {
    pub saves: &'a SaveStore,
    pub media: &'a mut MediaStore,
    pub rom_copies: &'a RomCopies,
    pub online: &'a mut Online,
    pub settings: &'a mut Settings,
    pub stats: &'a mut Stats,
    #[cfg(not(target_arch = "wasm32"))]
    pub recent: &'a mut RecentRoms,
    pub boot_roms: boot_roms::BootRoms<'a>,
    #[cfg(feature = "gba")]
    pub gba_bios: gba_bios::GbaBios<'a>,
    #[cfg(feature = "nds")]
    pub ds_files: ds_files::DsFiles<'a>,
    #[cfg(feature = "nds")]
    pub walker: &'a mut crate::walker::Pokewalker,
    #[cfg(ds_wifi)]
    pub ds_wifi: ds_wifi::DsWifiPart<'a>,
}

impl Parts<'_> {
    pub fn each(&mut self, mut visit: impl FnMut(&mut dyn Part)) {
        visit(&mut Saves(self.saves, Store::Saves));
        visit(&mut Saves(self.saves, Store::States));
        visit(&mut Media(&mut *self.media, Store::Screenshots));
        visit(&mut Media(&mut *self.media, Store::Recordings));
        visit(&mut StatsPart(&mut *self.stats));
        visit(&mut Chats(&mut *self.online));
        visit(&mut SettingsPart(&mut *self.settings));
        #[cfg(not(target_arch = "wasm32"))]
        visit(&mut Recent(&mut *self.recent));
        visit(&mut GameNames(&mut *self.settings));
        for file in <crate::app::boot_rom::BootRom as strum::IntoEnumIterator>::iter() {
            visit(&mut self.boot_roms.part(file));
        }
        #[cfg(feature = "gba")]
        visit(&mut self.gba_bios);
        #[cfg(feature = "nds")]
        for file in <crate::app::ds_firmware::DsFile as strum::IntoEnumIterator>::iter() {
            visit(&mut self.ds_files.part(file));
        }
        #[cfg(feature = "nds")]
        visit(&mut walker::WalkerPart(&mut *self.walker));
        #[cfg(ds_wifi)]
        visit(&mut self.ds_wifi);
        visit(&mut GameCopies(self.rom_copies));
        visit(&mut OtherData);
    }

    pub fn tallies(&mut self, usage: &Usage) -> Vec<(Kind, Tally)> {
        let mut counts: BTreeMap<Store, usize> = BTreeMap::new();
        let mut files = Vec::new();
        self.each(|part| {
            let tally = part.tally();
            *counts.entry(part.kind()).or_default() += tally.count;
            files.extend(part.file_row().map(|row| (row, tally)));
        });
        crate::storage::kinds::listed(Section::Data, usage)
            .flat_map(|kind| {
                if kind == Store::SystemFiles {
                    return files.clone();
                }
                let tally = Tally {
                    count: counts.get(&kind).copied().unwrap_or(0),
                };
                vec![(Kind::of(kind), tally)]
            })
            .collect()
    }

    pub fn export(&mut self) -> Files {
        let mut files = Files::new();
        self.each(|part| part.export(&mut files));
        files
    }

    pub fn plan(&mut self, files: &Files) -> Vec<(Kind, Change)> {
        let mut changes: Vec<(Store, Change)> = Vec::new();
        self.each(|part| {
            let change = part.plan(files);
            match changes.iter_mut().find(|(kind, _)| *kind == part.kind()) {
                Some((_, total)) => {
                    total.added += change.added;
                    total.replaced += change.replaced;
                }
                None => changes.push((part.kind(), change)),
            }
        });
        changes
            .into_iter()
            .map(|(kind, change)| (Kind::of(kind), change))
            .collect()
    }

    pub fn import(&mut self, files: &Files) -> io::Result<()> {
        let mut result = Ok(());
        self.each(|part| {
            if let Err(err) = part.import(files) {
                log::error!("Could not import the {}: {err}", part.kind().name());
                result = Err(err);
            }
        });
        result
    }

    pub fn delete_all(&mut self) -> io::Result<()> {
        let mut result = Ok(());
        self.each(|part| {
            if let Err(err) = part.delete_all() {
                log::error!("Could not delete the {}: {err}", part.kind().name());
                result = Err(err);
            }
        });
        result
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReadError {
    NotZip,
    NotOurs,
    Newer,
    TooLarge,
}

impl ReadError {
    pub fn text(&self) -> &'static str {
        match self {
            Self::NotZip | Self::NotOurs => "This file isn't a Citrine data export.",
            Self::Newer => "This export is from a newer Citrine. Update Citrine to import it.",
            Self::TooLarge => "This export is too large to open.",
        }
    }
}

#[cfg(test)]
pub fn pack(files: &Files) -> io::Result<Vec<u8>> {
    let mut packer = Packer::new(Counter::default());
    packer.add_all(files);
    packer.finish()
}

pub fn export_bytes(files: &Files, media: &[crate::media::MediaItem]) -> u64 {
    let files: u64 = files.values().map(|bytes| bytes.len() as u64).sum();
    files + media.iter().map(|item| item.info.bytes).sum::<u64>()
}

pub struct Packer {
    zip: zip::ZipWriter<io::Cursor<Vec<u8>>>,
    counter: Counter,
    failed: Option<io::Error>,
}

impl Packer {
    pub fn new(counter: Counter) -> Self {
        let mut packer = Self {
            zip: zip::ZipWriter::new(io::Cursor::new(Vec::new())),
            counter,
            failed: None,
        };
        let manifest = serde_json::json!({
            "format": FORMAT,
            "made_by": format!("Citrine {}", env!("CARGO_PKG_VERSION")),
            "made_at": crate::storage::now_unix(),
        });
        match serde_json::to_vec_pretty(&manifest) {
            Ok(bytes) => packer.write(MANIFEST, &bytes),
            Err(err) => packer.failed = Some(io::Error::other(err)),
        }
        packer
    }

    pub fn add_all(&mut self, files: &Files) {
        for (path, bytes) in files {
            self.add(path, bytes);
        }
    }

    pub fn add_media(&mut self, id: &MediaId, bytes: &[u8]) {
        self.add(&format!("{MEDIA}{}", id.as_str()), bytes);
    }

    fn add(&mut self, path: &str, bytes: &[u8]) {
        self.write(path, bytes);
        self.counter.add(bytes.len() as u64);
    }

    fn write(&mut self, path: &str, bytes: &[u8]) {
        if self.failed.is_some() {
            return;
        }
        let packed = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .large_file(true);
        let options = if already_packed(path) {
            packed.compression_method(zip::CompressionMethod::Stored)
        } else {
            packed
        };
        let result = self
            .zip
            .start_file(path, options)
            .map_err(io::Error::other)
            .and_then(|()| self.zip.write_all(bytes));
        if let Err(err) = result {
            self.failed = Some(err);
        }
    }

    pub fn finish(self) -> io::Result<Vec<u8>> {
        if let Some(err) = self.failed {
            return Err(err);
        }
        Ok(self.zip.finish().map_err(io::Error::other)?.into_inner())
    }
}

fn already_packed(path: &str) -> bool {
    [".png", ".mp4", ".snap", ".thumb", ".bin"]
        .iter()
        .any(|ext| path.ends_with(ext))
}

#[cfg(test)]
pub fn unpack(bytes: &[u8]) -> Result<Files, ReadError> {
    unpack_counted(bytes.into(), &Counter::default())
}

#[cfg(any(test, not(target_arch = "wasm32")))]
pub fn unpack_counted(bytes: std::sync::Arc<[u8]>, counter: &Counter) -> Result<Files, ReadError> {
    let mut unpacker = Unpacker::new(bytes, counter.clone())?;
    while unpacker.step_for(STEP_BUDGET)? {}
    Ok(unpacker.finish())
}

pub struct Unpacker {
    zip: zip::ZipArchive<io::Cursor<std::sync::Arc<[u8]>>>,
    next: usize,
    total: u64,
    files: Files,
    counter: Counter,
}

impl Unpacker {
    pub fn new(bytes: std::sync::Arc<[u8]>, counter: Counter) -> Result<Self, ReadError> {
        let mut zip =
            zip::ZipArchive::new(io::Cursor::new(bytes)).map_err(|_| ReadError::NotZip)?;
        let manifest: Value = {
            let file = zip.by_name(MANIFEST).map_err(|_| ReadError::NotOurs)?;
            let mut raw = Vec::new();
            file.take(MAX_MANIFEST_BYTES + 1)
                .read_to_end(&mut raw)
                .map_err(|_| ReadError::NotOurs)?;
            if raw.len() as u64 > MAX_MANIFEST_BYTES {
                return Err(ReadError::NotOurs);
            }
            serde_json::from_slice(&raw).map_err(|_| ReadError::NotOurs)?
        };
        let format = manifest["format"].as_u64().ok_or(ReadError::NotOurs)?;
        if format > FORMAT {
            return Err(ReadError::Newer);
        }
        let sizes = (0..zip.len()).filter_map(|i| zip.by_index_raw(i).ok().map(|f| f.size()));
        counter.set_total(sizes.fold(0u64, u64::saturating_add));
        Ok(Self {
            zip,
            next: 0,
            total: 0,
            files: Files::new(),
            counter,
        })
    }

    pub fn step_for(&mut self, budget: std::time::Duration) -> Result<bool, ReadError> {
        let until = web_time::Instant::now() + budget;
        while self.step()? {
            if web_time::Instant::now() >= until {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn step(&mut self) -> Result<bool, ReadError> {
        if self.next >= self.zip.len() {
            return Ok(false);
        }
        let mut file = self
            .zip
            .by_index(self.next)
            .map_err(|_| ReadError::NotZip)?;
        self.next += 1;
        let name = file.name().map_err(|_| ReadError::NotZip)?.into_owned();
        if file.is_dir() || name == MANIFEST {
            self.counter.add(file.size());
            return Ok(true);
        }
        let room = entry_cap(&name).min(MAX_FILE_BYTES - self.total);
        if file.size() > room {
            return Err(ReadError::TooLarge);
        }
        let mut raw = Vec::with_capacity(file.size() as usize);
        file.by_ref()
            .take(room + 1)
            .read_to_end(&mut raw)
            .map_err(|_| ReadError::NotZip)?;
        if raw.len() as u64 > room {
            return Err(ReadError::TooLarge);
        }
        self.total += raw.len() as u64;
        self.counter.add(raw.len() as u64);
        self.files.insert(name, raw);
        Ok(true)
    }

    pub fn finish(self) -> Files {
        self.files
    }
}

fn entry_cap(path: &str) -> u64 {
    let media =
        path.starts_with(MEDIA) && !path.ends_with(INFO_SUFFIX) && !path.ends_with(THUMB_SUFFIX);
    if media {
        MAX_FILE_BYTES
    } else {
        MAX_ENTRY_BYTES
    }
}

pub fn file_name(now: u64) -> String {
    format!(
        "citrine-data-{}.{FILE_EXTENSION}",
        crate::media::local_stamp(now, "%Y-%m-%d")
    )
}

struct Saves<'a>(&'a SaveStore, Store);

fn save_kind(name: &str) -> Store {
    Folder::Saves.kind_of(name)
}

type SaveItem = (String, String);
type Incoming<'f> = (SaveItem, Vec<(&'f str, &'f [u8])>, Option<Vec<String>>);

fn save_item(rom: &str, name: &str) -> SaveItem {
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    (rom.to_owned(), stem.to_owned())
}

fn saved_at(meta: Option<&[u8]>) -> u64 {
    meta.and_then(|m| m.get(..8))
        .and_then(|b| b.try_into().ok())
        .map_or(0, u64::from_le_bytes)
}

fn file_items(files: &Files, kind: Store) -> BTreeMap<SaveItem, Vec<(&str, &[u8])>> {
    let mut items: BTreeMap<SaveItem, Vec<(&str, &[u8])>> = BTreeMap::new();
    for (path, bytes) in files.range(SAVES.to_owned()..) {
        let Some(rest) = path.strip_prefix(SAVES) else {
            break;
        };
        let Some((rom, name)) = rest.split_once('/') else {
            continue;
        };
        if !crate::storage::is_save_key(rom)
            || !crate::storage::is_user_file(name)
            || save_kind(name) != kind
        {
            continue;
        }
        items
            .entry(save_item(rom, name))
            .or_default()
            .push((name, bytes.as_slice()));
    }
    items
}

impl Saves<'_> {
    fn files(&self) -> Vec<(String, String)> {
        let mut files = self.0.files();
        files.retain(|(_, name)| save_kind(name) == self.1);
        files
    }

    fn local_items(&self) -> BTreeMap<SaveItem, Vec<String>> {
        let mut items: BTreeMap<SaveItem, Vec<String>> = BTreeMap::new();
        for (rom, name) in self.files() {
            items.entry(save_item(&rom, &name)).or_default().push(name);
        }
        items
    }

    fn local_saved_at(&self, (rom, stem): &SaveItem) -> u64 {
        saved_at(self.0.read_file(rom, &format!("{stem}.meta")).as_deref())
    }

    fn incoming<'f>(&self, files: &'f Files) -> Vec<Incoming<'f>> {
        let mut local = self.local_items();
        file_items(files, self.1)
            .into_iter()
            .filter_map(|(item, theirs)| {
                let mine = local.remove(&item);
                if mine.is_some() {
                    let their_time = saved_at(
                        theirs
                            .iter()
                            .find(|(name, _)| name.ends_with(".meta"))
                            .map(|(_, bytes)| *bytes),
                    );
                    if their_time <= self.local_saved_at(&item) {
                        return None;
                    }
                }
                Some((item, theirs, mine))
            })
            .collect()
    }
}

impl Part for Saves<'_> {
    fn kind(&self) -> Store {
        self.1
    }

    fn tally(&mut self) -> Tally {
        Tally {
            count: self.local_items().len(),
        }
    }

    fn export(&mut self, out: &mut Files) {
        for (rom, name) in self.files() {
            if let Some(bytes) = self.0.read_file(&rom, &name) {
                out.insert(format!("{SAVES}{rom}/{name}"), bytes);
            }
        }
    }

    fn plan(&mut self, files: &Files) -> Change {
        let mut change = Change::default();
        for (_, _, mine) in self.incoming(files) {
            match mine {
                Some(_) => change.replaced += 1,
                None => change.added += 1,
            }
        }
        change
    }

    fn import(&mut self, files: &Files) -> io::Result<()> {
        for ((rom, _), theirs, mine) in self.incoming(files) {
            for name in mine.unwrap_or_default() {
                self.0.remove_file(&rom, &name)?;
            }
            for (name, bytes) in theirs {
                self.0.put_file(&rom, name, bytes)?;
            }
        }
        Ok(())
    }

    fn delete_all(&mut self) -> io::Result<()> {
        if self.1 == Store::Saves {
            return self.0.delete_every_game();
        }
        for (rom, name) in self.files() {
            self.0.remove_file(&rom, &name)?;
        }
        Ok(())
    }
}

struct Media<'a>(&'a mut MediaStore, Store);

fn media_kind(info: &MediaInfo) -> Store {
    Folder::Media.kind_of(info.kind.folder())
}

struct IncomingMedia<'f> {
    id: MediaId,
    info: MediaInfo,
    thumb: Option<&'f [u8]>,
    bytes: &'f [u8],
    replaces: bool,
}

impl Media<'_> {
    fn incoming<'f>(&self, files: &'f Files) -> Vec<IncomingMedia<'f>> {
        let mut found = Vec::new();
        for (path, bytes) in files.range(MEDIA.to_owned()..) {
            let Some(rest) = path.strip_prefix(MEDIA) else {
                break;
            };
            let Some(id) = MediaId::parse(rest) else {
                continue;
            };
            let Some(info) = files
                .get(&format!("{path}{INFO_SUFFIX}"))
                .and_then(|raw| serde_json::from_slice::<MediaInfo>(raw).ok())
            else {
                continue;
            };
            if media_kind(&info) != self.1 {
                continue;
            }
            let mine = self.0.items().iter().find(|item| item.id == id);
            if mine.is_some_and(|mine| mine.info.created_at >= info.created_at) {
                continue;
            }
            found.push(IncomingMedia {
                replaces: mine.is_some(),
                thumb: files
                    .get(&format!("{path}{THUMB_SUFFIX}"))
                    .map(Vec::as_slice),
                id,
                info,
                bytes,
            });
        }
        found
    }
}

impl Part for Media<'_> {
    fn kind(&self) -> Store {
        self.1
    }

    fn tally(&mut self) -> Tally {
        self.0.refresh();
        let count = self
            .0
            .items()
            .iter()
            .filter(|item| media_kind(&item.info) == self.1)
            .count();
        Tally { count }
    }

    fn export(&mut self, out: &mut Files) {
        let kind = self.1;
        for item in self
            .0
            .items()
            .iter()
            .filter(|item| media_kind(&item.info) == kind)
        {
            let path = format!("{MEDIA}{}", item.id.as_str());
            if let Ok(info) = serde_json::to_vec_pretty(&item.info) {
                out.insert(format!("{path}{INFO_SUFFIX}"), info);
            }
            if let Some(png) = item
                .thumbnail
                .as_ref()
                .and_then(crate::media::encode_thumbnail)
            {
                out.insert(format!("{path}{THUMB_SUFFIX}"), png);
            }
        }
    }

    fn plan(&mut self, files: &Files) -> Change {
        let mut change = Change::default();
        for incoming in self.incoming(files) {
            if incoming.replaces {
                change.replaced += 1;
            } else {
                change.added += 1;
            }
        }
        change
    }

    fn import(&mut self, files: &Files) -> io::Result<()> {
        let incoming: Vec<_> = self
            .incoming(files)
            .into_iter()
            .map(|m| (m.id, m.info, m.thumb, m.bytes))
            .collect();
        for (id, info, thumb, bytes) in incoming {
            self.0.restore(&id, info, thumb, bytes)?;
        }
        Ok(())
    }

    fn delete_all(&mut self) -> io::Result<()> {
        self.0.delete_all()
    }
}

struct Chats<'a>(&'a mut Online);

impl Chats<'_> {
    fn merged(&mut self, files: &Files) -> Vec<(String, Vec<u8>, usize)> {
        let local: BTreeMap<String, Vec<u8>> = self.0.chat_files().into_iter().collect();
        let mut out = Vec::new();
        for (path, bytes) in files.range(CHATS.to_owned()..) {
            let Some(name) = path.strip_prefix(CHATS) else {
                break;
            };
            let Some(theirs) = decode_history(bytes) else {
                continue;
            };
            let mut mine = local
                .get(name)
                .and_then(|raw| decode_history(raw))
                .unwrap_or_default();
            let added = merge_history(&mut mine, &theirs);
            if added == 0 {
                continue;
            }
            if let Some(encoded) = encode_history(&mine) {
                out.push((name.to_owned(), encoded, added));
            }
        }
        out
    }
}

impl Part for Chats<'_> {
    fn kind(&self) -> Store {
        Store::Chats
    }

    fn tally(&mut self) -> Tally {
        let files = self.0.chat_files();
        Tally {
            count: files
                .iter()
                .filter_map(|(_, raw)| decode_history(raw))
                .flat_map(|history| history.into_values())
                .map(|chat| chat.entries.len())
                .sum(),
        }
    }

    fn export(&mut self, out: &mut Files) {
        for (name, bytes) in self.0.chat_files() {
            out.insert(format!("{CHATS}{name}"), bytes);
        }
    }

    fn plan(&mut self, files: &Files) -> Change {
        Change {
            added: self.merged(files).iter().map(|(_, _, added)| added).sum(),
            replaced: 0,
        }
    }

    fn import(&mut self, files: &Files) -> io::Result<()> {
        for (name, bytes, _) in self.merged(files) {
            self.0.chat_restore(&name, &bytes)?;
        }
        Ok(())
    }

    fn delete_all(&mut self) -> io::Result<()> {
        self.0.chat_delete_all()
    }
}

struct SettingsPart<'a>(&'a mut Settings);

fn settings_json(settings: &Settings) -> serde_json::Map<String, Value> {
    let Ok(Value::Object(mut map)) = serde_json::to_value(settings) else {
        return Default::default();
    };
    for key in KEPT_SETTINGS {
        map.remove(*key);
    }
    map
}

fn leaves(prefix: &str, value: &Value, out: &mut BTreeMap<String, Value>) {
    match value {
        Value::Object(map) if !map.is_empty() => {
            for (key, value) in map {
                leaves(&format!("{prefix}{key}."), value, out);
            }
        }
        other => {
            out.insert(prefix.trim_end_matches('.').to_owned(), other.clone());
        }
    }
}

fn differences(a: &serde_json::Map<String, Value>, b: &serde_json::Map<String, Value>) -> usize {
    let (mut left, mut right) = (BTreeMap::new(), BTreeMap::new());
    leaves("", &Value::Object(a.clone()), &mut left);
    leaves("", &Value::Object(b.clone()), &mut right);
    let keys: std::collections::BTreeSet<&String> = left.keys().chain(right.keys()).collect();
    keys.into_iter()
        .filter(|key| left.get(*key) != right.get(*key))
        .count()
}

fn file_settings(files: &Files) -> Option<serde_json::Map<String, Value>> {
    match serde_json::from_slice(files.get(SETTINGS)?).ok()? {
        Value::Object(mut map) => {
            for key in KEPT_SETTINGS {
                map.remove(*key);
            }
            Some(map)
        }
        _ => None,
    }
}

impl Part for SettingsPart<'_> {
    fn kind(&self) -> Store {
        Store::Settings
    }

    fn tally(&mut self) -> Tally {
        let mine = settings_json(self.0);
        let defaults = settings_json(&Settings::default());
        Tally {
            count: differences(&mine, &defaults),
        }
    }

    fn export(&mut self, out: &mut Files) {
        if let Ok(raw) = serde_json::to_vec_pretty(&settings_json(self.0)) {
            out.insert(SETTINGS.to_owned(), raw);
        }
    }

    fn plan(&mut self, files: &Files) -> Change {
        let Some(theirs) = file_settings(files) else {
            return Change::default();
        };
        let Some(adopted) = adopt(Value::Object(settings_json(self.0)), theirs) else {
            return Change::default();
        };
        Change {
            added: 0,
            replaced: differences(&settings_json(&adopted), &settings_json(self.0)),
        }
    }

    fn import(&mut self, files: &Files) -> io::Result<()> {
        let Some(theirs) = file_settings(files) else {
            return Ok(());
        };
        let mine = serde_json::to_value(&*self.0).map_err(io::Error::other)?;
        let mut adopted =
            adopt(mine, theirs).ok_or_else(|| io::Error::other("settings unreadable"))?;
        adopted.cheats.turn_off_unknown(&self.0.cheats);
        adopted.sanitize();
        adopted.dirty = true;
        *self.0 = adopted;
        Ok(())
    }

    fn delete_all(&mut self) -> io::Result<()> {
        let online = std::mem::take(&mut self.0.online);
        let current_tab = self.0.current_tab;
        *self.0 = Settings {
            online,
            current_tab,
            dirty: true,
            ..Settings::default()
        };
        Ok(())
    }
}

fn adopt(mine: Value, theirs: serde_json::Map<String, Value>) -> Option<Settings> {
    let Value::Object(mut merged) = mine.clone() else {
        return None;
    };
    merged.extend(theirs);
    salvage::<Settings>(Value::Object(merged), Some(&mine)).map(|kept| kept.value)
}

struct GameNames<'a>(&'a mut Settings);

impl GameNames<'_> {
    fn incoming(&self, files: &Files) -> Vec<(String, String, bool)> {
        let Some(theirs) = files
            .get(GAME_NAMES)
            .and_then(|raw| serde_json::from_slice::<BTreeMap<String, String>>(raw).ok())
        else {
            return Vec::new();
        };
        let mine: BTreeMap<&str, &str> = self.0.game_names.iter().collect();
        theirs
            .into_iter()
            .filter_map(|(key, name)| match mine.get(key.as_str()) {
                Some(known) if *known == name => None,
                known => Some((key, name, known.is_some())),
            })
            .collect()
    }
}

impl Part for GameNames<'_> {
    fn kind(&self) -> Store {
        Store::GameNames
    }

    fn tally(&mut self) -> Tally {
        let names: BTreeMap<&str, &str> = self.0.game_names.iter().collect();
        Tally { count: names.len() }
    }

    fn export(&mut self, out: &mut Files) {
        let names: BTreeMap<&str, &str> = self.0.game_names.iter().collect();
        if let Ok(raw) = serde_json::to_vec_pretty(&names) {
            out.insert(GAME_NAMES.to_owned(), raw);
        }
    }

    fn plan(&mut self, files: &Files) -> Change {
        let incoming = self.incoming(files);
        let replaced = incoming.iter().filter(|(_, _, known)| *known).count();
        Change {
            added: incoming.len() - replaced,
            replaced,
        }
    }

    fn import(&mut self, files: &Files) -> io::Result<()> {
        for (key, name, _) in self.incoming(files) {
            self.0.dirty |= self.0.game_names.learn(&key, &name);
        }
        Ok(())
    }

    fn delete_all(&mut self) -> io::Result<()> {
        self.0.game_names = Default::default();
        self.0.dirty = true;
        Ok(())
    }
}

struct StatsPart<'a>(&'a mut Stats);

impl StatsPart<'_> {
    fn portable(book: &Book) -> Book {
        Book {
            device: String::new(),
            devices: book.devices.clone(),
        }
    }

    fn games(book: &Book) -> BTreeMap<&str, Vec<(&str, &GameRecord)>> {
        let mut games: BTreeMap<&str, Vec<_>> = BTreeMap::new();
        for (device, record) in &book.devices {
            for (rom, game) in &record.games {
                games
                    .entry(rom.as_str())
                    .or_default()
                    .push((device.as_str(), game));
            }
        }
        games
    }

    fn theirs(files: &Files) -> Option<Book> {
        files
            .get(STATS)
            .and_then(|raw| serde_json::from_slice(raw).ok())
    }
}

impl Part for StatsPart<'_> {
    fn kind(&self) -> Store {
        Store::Stats
    }

    fn tally(&mut self) -> Tally {
        let book = self.0.book();
        if book.is_empty() {
            return Tally::default();
        }
        Tally {
            count: Self::games(book).len(),
        }
    }

    fn export(&mut self, out: &mut Files) {
        let book = self.0.book();
        if book.is_empty() {
            return;
        }
        if let Ok(raw) = serde_json::to_vec_pretty(&Self::portable(book)) {
            out.insert(STATS.to_owned(), raw);
        }
    }

    fn plan(&mut self, files: &Files) -> Change {
        let Some(theirs) = Self::theirs(files) else {
            return Change::default();
        };
        let mine = self.0.book();
        let mut merged = mine.clone();
        merged.merge(&theirs);
        let before = Self::games(mine);
        let after = Self::games(&merged);
        Change {
            added: after
                .iter()
                .filter(|(rom, rows)| before.get(*rom) != Some(*rows))
                .count(),
            replaced: 0,
        }
    }

    fn import(&mut self, files: &Files) -> io::Result<()> {
        let Some(raw) = files.get(STATS) else {
            return Ok(());
        };
        self.0
            .import(raw)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    }

    fn delete_all(&mut self) -> io::Result<()> {
        self.0.delete_all()
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct Recent<'a>(&'a mut RecentRoms);

#[cfg(not(target_arch = "wasm32"))]
fn network_path(path: &std::path::Path) -> bool {
    path.as_os_str()
        .to_string_lossy()
        .chars()
        .take_while(|c| matches!(c, '/' | '\\'))
        .nth(1)
        .is_some()
}

#[cfg(not(target_arch = "wasm32"))]
impl Recent<'_> {
    fn theirs(files: &Files) -> Vec<RecentRom> {
        files
            .get(RECENT)
            .and_then(|raw| serde_json::from_slice::<Vec<RecentRom>>(raw).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|rom| !network_path(&rom.path))
            .collect()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Part for Recent<'_> {
    fn kind(&self) -> Store {
        Store::RecentGames
    }

    fn tally(&mut self) -> Tally {
        Tally {
            count: self.0.entries().len(),
        }
    }

    fn export(&mut self, out: &mut Files) {
        if let Ok(raw) = serde_json::to_vec_pretty(self.0.entries()) {
            out.insert(RECENT.to_owned(), raw);
        }
    }

    fn plan(&mut self, files: &Files) -> Change {
        let (_, added, replaced) = self.0.merged(&Self::theirs(files));
        Change { added, replaced }
    }

    fn import(&mut self, files: &Files) -> io::Result<()> {
        let (entries, _, _) = self.0.merged(&Self::theirs(files));
        self.0.replace(entries);
        Ok(())
    }

    fn delete_all(&mut self) -> io::Result<()> {
        self.0.clear();
        Ok(())
    }
}

struct GameCopies<'a>(&'a RomCopies);

impl Part for GameCopies<'_> {
    fn kind(&self) -> Store {
        Store::GameCopies
    }

    fn tally(&mut self) -> Tally {
        Tally {
            count: self.0.count(),
        }
    }

    fn export(&mut self, _: &mut Files) {}

    fn plan(&mut self, _: &Files) -> Change {
        Change::default()
    }

    fn import(&mut self, _: &Files) -> io::Result<()> {
        Ok(())
    }

    fn delete_all(&mut self) -> io::Result<()> {
        self.0.clear();
        Ok(())
    }
}

const OTHER_FILES: &str = "other/";

struct OtherData;

impl OtherData {
    fn theirs(files: &Files) -> Vec<(&str, &[u8])> {
        files
            .range(OTHER_FILES.to_owned()..)
            .map_while(|(path, bytes)| Some((path.strip_prefix(OTHER_FILES)?, bytes.as_slice())))
            .filter(|(path, _)| other_data::is_other(path))
            .collect()
    }
}

impl Part for OtherData {
    fn kind(&self) -> Store {
        Store::OtherData
    }

    fn tally(&mut self) -> Tally {
        Tally {
            count: other_data::list().len(),
        }
    }

    fn export(&mut self, out: &mut Files) {
        for path in other_data::list() {
            if let Some(bytes) = other_data::read(&path) {
                out.insert(format!("{OTHER_FILES}{path}"), bytes);
            }
        }
    }

    fn plan(&mut self, files: &Files) -> Change {
        let mut change = Change::default();
        for (path, bytes) in Self::theirs(files) {
            match other_data::read(path) {
                None => change.added += 1,
                Some(mine) if mine != bytes => change.replaced += 1,
                Some(_) => {}
            }
        }
        change
    }

    fn import(&mut self, files: &Files) -> io::Result<()> {
        for (path, bytes) in Self::theirs(files) {
            other_data::write(path, bytes)?;
        }
        Ok(())
    }

    fn delete_all(&mut self) -> io::Result<()> {
        for path in other_data::list() {
            other_data::delete(&path)?;
        }
        Ok(())
    }
}

pub mod boot_roms;
#[cfg(feature = "nds")]
pub mod ds_files;
#[cfg(ds_wifi)]
pub mod ds_wifi;
#[cfg(feature = "gba")]
pub mod gba_bios;
pub mod kept_file;
#[cfg(feature = "nds")]
mod walker;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::media::{Frame, MediaKind, NewMedia};
    use crate::online::chat::history::History;
    use crate::online::chat::{ChatEntry, Conversation, Delivery};
    use crate::online::settings::RelayEntry;
    use citrine_gb_net::{Body, OnlineId};
    use std::path::{Path, PathBuf};

    const GAME: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const OTHER: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
    const HISTORY: &str = "1234_relay_example.bin";
    const BORROWED: &str = "borrowed-00112233445566778899aabbccddeeff00112233";

    struct Home {
        root: PathBuf,
        saves: SaveStore,
        media: MediaStore,
        rom_copies: RomCopies,
        online: Online,
        settings: Settings,
        stats: Stats,
        recent: RecentRoms,
        boot: (
            crate::storage::SettingsStore,
            citrine_gb::machine::Firmware,
            [String; 2],
        ),
        #[cfg(feature = "gba")]
        bios: (
            crate::storage::SettingsStore,
            Option<citrine_gb::gba::bios::Bios>,
            String,
        ),
        #[cfg(feature = "nds")]
        ds: (citrine_gb::nds::machine::NdsFirmware, [String; 3]),
        #[cfg(feature = "nds")]
        walker: crate::walker::Pokewalker,
        #[cfg(ds_wifi)]
        wifi: Option<crate::ds_wifi::Identity>,
    }

    impl Home {
        fn new(name: &str) -> Self {
            let root = crate::storage::scratch_dir()
                .join(format!("citrine-your-data-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            let mut online = Online::default();
            online.persist_chats_in(root.join("data"));
            Self {
                saves: SaveStore::at(root.join("saves")),
                media: MediaStore::at(root.join("media")),
                rom_copies: RomCopies::at(root.join("roms")),
                online,
                settings: Settings::default(),
                stats: Stats::open(crate::stats::StatsStore::in_dir(root.join("stats"))),
                recent: RecentRoms::default(),
                boot: (
                    crate::storage::SettingsStore::at(root.join("settings")),
                    Default::default(),
                    Default::default(),
                ),
                #[cfg(feature = "gba")]
                bios: (
                    crate::storage::SettingsStore::at(root.join("settings")),
                    None,
                    String::new(),
                ),
                #[cfg(feature = "nds")]
                ds: Default::default(),
                #[cfg(feature = "nds")]
                walker: crate::walker::Pokewalker::new(Box::new(crate::walker::FileStore::at(
                    root.join("settings"),
                ))),
                #[cfg(ds_wifi)]
                wifi: None,
                root,
            }
        }

        fn parts(&mut self) -> Parts<'_> {
            Parts {
                saves: &self.saves,
                media: &mut self.media,
                rom_copies: &self.rom_copies,
                online: &mut self.online,
                settings: &mut self.settings,
                stats: &mut self.stats,
                recent: &mut self.recent,
                boot_roms: boot_roms::BootRoms {
                    store: &self.boot.0,
                    gb: &mut self.boot.1.gb_boot_rom,
                    gbc: &mut self.boot.1.gbc_boot_rom,
                    names: &mut self.boot.2,
                },
                #[cfg(feature = "gba")]
                gba_bios: gba_bios::GbaBios {
                    store: &self.bios.0,
                    bios: &mut self.bios.1,
                    name: &mut self.bios.2,
                },
                #[cfg(feature = "nds")]
                ds_files: ds_files::DsFiles {
                    store: &self.bios.0,
                    slots: &mut self.ds.0,
                    names: &mut self.ds.1,
                },
                #[cfg(feature = "nds")]
                walker: &mut self.walker,
                #[cfg(ds_wifi)]
                ds_wifi: ds_wifi::DsWifiPart {
                    store: &self.bios.0,
                    identity: &mut self.wifi,
                },
            }
        }

        fn fill(&mut self) {
            save(&self.saves, GAME, "battery", 100, b"battery");
            save(&self.saves, GAME, "slot0", 110, b"snapshot");
            save(&self.saves, OTHER, "auto3", 120, b"autosave");
            save(&self.saves, BORROWED, "battery", 130, b"borrowed");
            self.saves
                .put_file(GAME, "slot0.name", b"Before the gym")
                .unwrap();
            self.media
                .add_screenshot(new_media(MediaKind::Screenshot, 500), &png())
                .unwrap();
            self.media
                .add_file(new_media(MediaKind::Recording, 600), b"mp4 bytes")
                .unwrap();
            self.online
                .chat_restore(
                    HISTORY,
                    &encode_history(&history(&[(1, 10), (2, 20)])).unwrap(),
                )
                .unwrap();
            play(&mut self.stats, GAME, 90);
            self.settings.volume = 0.5;
            self.settings.matrix = true;
            self.settings.game_names.learn(GAME, "Pokémon Silver");
            self.recent
                .record(Path::new("/roms/silver.gbc"), "Pokémon Silver", None);
            sign_in(&mut self.settings);
            self.boot.1.gb_boot_rom = Some((0..0x100).map(|n| n as u8).collect());
            self.boot.1.gbc_boot_rom = Some((0..0x900).map(|n| (n * 3) as u8).collect());
            self.boot.2 = ["dmg_boot.bin".to_owned(), "cgb_boot.bin".to_owned()];
            #[cfg(feature = "gba")]
            {
                self.bios.1 = Some(
                    citrine_gb::gba::bios::Bios::user(citrine_gb::gba::bios::CULT_OF_GBA).unwrap(),
                );
                self.bios.2 = "gba_bios.bin".to_owned();
            }
            #[cfg(feature = "nds")]
            {
                self.ds.0.arm9_bios = Some((0..0x1000).map(|n| n as u8).collect());
                self.ds.0.arm7_bios = Some((0..0x4000).map(|n| (n * 7) as u8).collect());
                self.ds.0.firmware = Some((0..0x4_0000).map(|n| (n * 13) as u8).collect());
                self.ds.1 = [
                    "biosnds9.rom".to_owned(),
                    "biosnds7.rom".to_owned(),
                    "firmware.bin".to_owned(),
                ];
                self.walker.press(citrine_gb_walker::Button::Middle);
            }
            #[cfg(ds_wifi)]
            {
                let mut identity = crate::ds_wifi::Identity::random();
                identity.connections = Some(
                    (0..citrine_gb::nds::spi::firmware::online::CONNECTIONS_BYTES)
                        .map(|n| n as u8)
                        .collect(),
                );
                self.wifi = Some(identity);
            }
        }
    }

    impl Drop for Home {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn sign_in(settings: &mut Settings) {
        settings.online.joined = Some("relay.example".into());
        settings.online.relays.insert(
            "relay.example".into(),
            RelayEntry {
                relay_id: "secret relay id".into(),
                password: "secret password".into(),
                online_id: Some("1234".into()),
                display_name: Some("Clara".into()),
                relay_key: None,
                device: None,
                others: Vec::new(),
                friend_keys: Default::default(),
            },
        );
    }

    fn play(stats: &mut Stats, rom: &str, secs: u64) {
        stats.recorder().play(
            rom,
            "Pokémon Silver",
            std::time::Duration::from_secs(secs),
            crate::stats::Activity::default(),
            1_000,
            "2026-10-31",
        );
    }

    fn save(store: &SaveStore, rom: &str, stem: &str, saved_at: u64, data: &[u8]) {
        let role = if stem.starts_with("battery") {
            "sav"
        } else {
            "snap"
        };
        store
            .put_file(rom, &format!("{stem}.{role}"), data)
            .unwrap();
        let mut meta = saved_at.to_le_bytes().to_vec();
        if role == "snap" {
            meta.push(1);
        }
        store.put_file(rom, &format!("{stem}.meta"), &meta).unwrap();
    }

    fn frame() -> Frame {
        Frame {
            width: 160,
            height: 144,
            rgba: (0..160 * 144 * 4).map(|i| (i % 251) as u8).collect(),
        }
    }

    fn png() -> Vec<u8> {
        let frame = frame();
        crate::utils::screenshot::encode_png(160, 144, &frame.rgba).unwrap()
    }

    fn new_media(kind: MediaKind, created_at: u64) -> NewMedia {
        NewMedia {
            kind,
            game: "Pokémon Silver".into(),
            rom_key: Some(GAME.into()),
            model: None,
            created_at,
            width: 160,
            height: 144,
            length_ms: (kind == MediaKind::Recording).then_some(4000),
            frame_times: false,
            video: false,
            replay: None,
            frame: frame(),
        }
    }

    fn history(entries: &[(u64, u64)]) -> History {
        let mut chat = Conversation::default();
        for (id, at) in entries {
            chat.entries.push(ChatEntry {
                id: *id,
                mine: id % 2 == 0,
                at: *at,
                body: Body::Text(format!("message {id}")),
                delivery: Delivery::Received,
                change: crate::online::chat::Change::Kept,
                reply: None,
            });
        }
        History::from([(OnlineId::new(7).unwrap(), chat)])
    }

    fn counts(home: &mut Home) -> Vec<(&'static str, usize)> {
        home.parts()
            .tallies(&Usage::new())
            .into_iter()
            .map(|(kind, tally)| (kind.name, tally.count))
            .collect()
    }

    fn export(home: &mut Home) -> Files {
        let mut parts = home.parts();
        let mut files = parts.export();
        let (tx, rx) = std::sync::mpsc::channel();
        home.media.read_files(&spawner(), move |media| {
            let _ = tx.send(media);
        });
        while let Ok(Some((id, bytes))) = rx.recv() {
            files.insert(format!("{MEDIA}{}", id.as_str()), bytes);
        }
        files
    }

    fn through_zip(files: &Files) -> Files {
        unpack(&pack(files).unwrap()).unwrap()
    }

    #[test]
    fn export_then_delete_then_import_gives_the_same_data_back() {
        let mut home = Home::new("round-trip");
        home.fill();
        let before = export(&mut home);
        let counted = counts(&mut home);
        let mut wanted = vec![
            ("Saves", 2),
            ("Save states", 2),
            ("Screenshots", 1),
            ("Recordings", 1),
            ("Settings", 2),
            ("Game names", 1),
            ("Recent games", 1),
            ("GB boot ROM", 1),
            ("GBC boot ROM", 1),
        ];
        if cfg!(feature = "gba") {
            wanted.push(("GBA BIOS", 1));
        }
        if cfg!(feature = "nds") {
            wanted.extend([("DS ARM9 BIOS", 1), ("DS ARM7 BIOS", 1), ("DS firmware", 1)]);
        }
        if cfg!(ds_wifi) {
            wanted.push(("DS wifi connection", 1));
        }
        wanted.extend([("Stats", 1), ("Chats", 2)]);
        if cfg!(feature = "nds") {
            wanted.push(("Pokéwalker", 1));
        }
        wanted.push(("Game copies", 0));
        assert_eq!(counted, wanted);

        let file = through_zip(&before);
        assert_eq!(file, before, "the zip holds every file as it was");
        home.parts().delete_all().unwrap();
        assert!(
            counts(&mut home).iter().all(|(_, count)| *count == 0),
            "{:?}",
            counts(&mut home)
        );
        assert!(home.settings.online.joined.is_some(), "the account stays");
        for file in <crate::app::boot_rom::BootRom as strum::IntoEnumIterator>::iter() {
            assert_eq!(
                home.boot.0.kept_file(file.store_keys()),
                None,
                "{file:?} stayed"
            );
        }
        #[cfg(feature = "nds")]
        for file in <crate::app::ds_firmware::DsFile as strum::IntoEnumIterator>::iter() {
            assert_eq!(
                home.bios.0.kept_file(file.store_keys()),
                None,
                "{file:?} stayed"
            );
        }
        let plan = home.parts().plan(&file);
        assert!(
            plan.iter()
                .any(|(kind, change)| kind.name == "Stats" && change.added == 1),
            "{plan:?}"
        );

        home.parts().import(&file).unwrap();
        assert_eq!(counts(&mut home), counted);
        let after = export(&mut home);
        assert_eq!(
            after.keys().collect::<Vec<_>>(),
            before.keys().collect::<Vec<_>>()
        );
        for (path, bytes) in &before {
            assert_eq!(&after[path], bytes, "{path} came back changed");
        }
        assert_eq!(
            home.saves.read_file(GAME, "slot0.snap").unwrap(),
            b"snapshot"
        );
        #[cfg(feature = "nds")]
        assert_eq!(
            home.bios
                .0
                .kept_file(crate::app::ds_firmware::DsFile::Firmware.store_keys()),
            Some((
                before["ds-firmware/firmware.bin"].clone(),
                "firmware.bin".to_owned()
            ))
        );
    }

    #[test]
    fn the_export_leaves_out_the_online_account() {
        let mut home = Home::new("no-account");
        home.fill();
        let files = export(&mut home);
        let text = String::from_utf8_lossy(&files[SETTINGS]).into_owned();
        assert!(!text.contains("secret"), "{text}");
        assert!(!text.contains("relay.example"), "{text}");
        assert!(files.keys().all(|path| !path.contains(".cloud")));
    }

    #[test]
    fn the_export_and_the_save_list_leave_out_the_covers() {
        let mut home = Home::new("no-covers");
        home.fill();
        home.saves.covers().put_art("ABC123", &png()).unwrap();
        let files = export(&mut home);
        assert!(
            files.keys().all(|path| !path.contains("cover")),
            "{:?}",
            files.keys()
        );
        assert!(
            home.saves
                .games()
                .iter()
                .all(|game| !game.contains("cover"))
        );
        assert!(
            home.saves
                .files()
                .iter()
                .all(|(_, name)| !name.ends_with(".png"))
        );
    }

    #[test]
    fn import_adds_what_is_missing_and_keeps_the_newer_copy() {
        let mut home = Home::new("merge");
        save(&home.saves, GAME, "battery", 200, b"newer here");
        save(&home.saves, GAME, "slot0", 100, b"older here");
        home.online
            .chat_restore(HISTORY, &encode_history(&history(&[(1, 10)])).unwrap())
            .unwrap();
        home.settings.volume = 0.9;
        play(&mut home.stats, GAME, 30);

        let mut other = Home::new("merge-file");
        save(&other.saves, GAME, "battery", 150, b"older in the file");
        save(&other.saves, GAME, "slot0", 300, b"newer in the file");
        save(&other.saves, GAME, "slot1", 50, b"only in the file");
        other
            .online
            .chat_restore(
                HISTORY,
                &encode_history(&history(&[(1, 10), (2, 20)])).unwrap(),
            )
            .unwrap();
        other.settings.volume = 0.3;
        play(&mut other.stats, GAME, 60);
        play(&mut other.stats, OTHER, 20);
        let file = through_zip(&export(&mut other));

        let plan: Vec<(&str, Change)> = home
            .parts()
            .plan(&file)
            .into_iter()
            .map(|(kind, change)| (kind.name, change))
            .collect();
        let of = |name: &str| plan.iter().find(|(n, _)| *n == name).unwrap().1;
        assert_eq!(of("Saves"), Change::default());
        assert_eq!(
            of("Save states"),
            Change {
                added: 1,
                replaced: 1
            }
        );
        assert_eq!(
            of("Chats"),
            Change {
                added: 1,
                replaced: 0
            }
        );
        assert_eq!(
            of("Stats"),
            Change {
                added: 2,
                replaced: 0
            }
        );
        assert_eq!(
            of("Settings"),
            Change {
                added: 0,
                replaced: 1
            }
        );

        home.parts().import(&file).unwrap();
        let read = |name: &str| home.saves.read_file(GAME, name).unwrap();
        assert_eq!(read("battery.sav"), b"newer here");
        assert_eq!(read("slot0.snap"), b"newer in the file");
        assert_eq!(read("slot1.snap"), b"only in the file");
        assert_eq!(home.settings.volume, 0.3, "settings come from the file");
        let today = "2026-10-31".parse().unwrap();
        assert_eq!(home.stats.book().overview(today).play_ms, 110_000);
        let chats = decode_history(&home.online.chat_files()[0].1).unwrap();
        assert_eq!(chats.values().next().unwrap().entries.len(), 2);

        assert!(
            home.parts()
                .plan(&file)
                .iter()
                .all(|(_, change)| *change == Change::default()),
            "a second import changes nothing"
        );
    }

    #[test]
    fn an_import_keeps_this_devices_network_settings() {
        let mut other = Home::new("network-file");
        other.settings.ds_online.service = crate::app::ui_state::ds_online::OnlineService::Other;
        other.settings.ds_online.dns = Some("203.0.113.9".into());
        other.settings.discord.enabled = true;
        other.settings.check_updates = false;
        let files = through_zip(&export(&mut other));

        let mut home = Home::new("network");
        home.parts().import(&files).unwrap();
        assert_eq!(home.settings.ds_online, Default::default());
        assert_eq!(
            home.settings.discord.enabled,
            Settings::default().discord.enabled
        );
        assert!(home.settings.check_updates);
    }

    #[test]
    fn imported_cheats_arrive_turned_off() {
        let cheat = |code: &str, on| crate::cheats::SavedCheat {
            name: code.to_owned(),
            code: code.to_owned(),
            on,
        };
        let mut other = Home::new("cheats-file");
        other.settings.cheats.add(GAME, cheat("01FF16D0", true));
        other.settings.cheats.add(GAME, cheat("01FF17D0", true));
        let files = through_zip(&export(&mut other));

        let mut home = Home::new("cheats");
        home.settings.cheats.add(GAME, cheat("01FF16D0", true));
        home.parts().import(&files).unwrap();
        let list = home.settings.cheats.list(GAME);
        assert_eq!(list.len(), 2);
        assert!(list.iter().find(|c| c.code == "01FF16D0").unwrap().on);
        assert!(!list.iter().find(|c| c.code == "01FF17D0").unwrap().on);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn network_paths_in_recent_games_are_dropped() {
        let mut other = Home::new("recent-file");
        for path in [
            "//evil.example/share/a.gb",
            r"\\evil.example\share\b.gb",
            "/roms/c.gb",
        ] {
            other.recent.record(Path::new(path), "Game", None);
        }
        let files = through_zip(&export(&mut other));

        let mut home = Home::new("recent");
        home.parts().import(&files).unwrap();
        let paths: Vec<_> = home
            .recent
            .entries()
            .iter()
            .map(|r| r.path.clone())
            .collect();
        assert_eq!(paths, [PathBuf::from("/roms/c.gb")]);
    }

    #[test]
    fn a_file_that_isnt_an_export_is_refused() {
        assert_eq!(unpack(b"not a zip"), Err(ReadError::NotZip));
        let mut zip = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
        zip.start_file("hello.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"hi").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        assert_eq!(unpack(&bytes), Err(ReadError::NotOurs));
    }

    #[test]
    fn a_huge_manifest_is_refused_before_it_is_read() {
        let deflated = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let manifest = |pad: u64| {
            let mut zip = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
            zip.start_file(MANIFEST, deflated).unwrap();
            zip.write_all(br#"{"format":1}"#).unwrap();
            zip.write_all(&vec![b' '; pad as usize]).unwrap();
            zip.finish().unwrap().into_inner()
        };
        assert_eq!(unpack(&manifest(0)), Ok(Files::new()));
        let huge = manifest(MAX_MANIFEST_BYTES);
        assert!(huge.len() < 4096, "{} bytes", huge.len());
        assert_eq!(unpack(&huge), Err(ReadError::NotOurs));
    }

    #[test]
    fn an_entry_past_its_cap_is_refused() {
        let mut files = Files::new();
        files.insert(
            SETTINGS.to_owned(),
            vec![b' '; MAX_ENTRY_BYTES as usize + 1],
        );
        let bytes = pack(&files).unwrap();
        files.clear();
        assert_eq!(unpack(&bytes), Err(ReadError::TooLarge));
        assert_eq!(entry_cap("media/screenshots/a.png"), MAX_FILE_BYTES);
        assert_eq!(entry_cap("media/screenshots/a.png.json"), MAX_ENTRY_BYTES);
        assert_eq!(entry_cap("saves/game/battery.sav"), MAX_ENTRY_BYTES);
    }

    #[test]
    fn paths_from_the_file_cannot_leave_their_folder() {
        let mut home = Home::new("paths");
        let mut files = Files::new();
        files.insert(format!("{SAVES}{GAME}/../../escape.sav"), b"x".to_vec());
        files.insert(format!("{SAVES}../{GAME}/battery.sav"), b"x".to_vec());
        files.insert(format!("{SAVES}{GAME}/battery.cloud"), b"x".to_vec());
        files.insert(format!("{MEDIA}screenshots/../x.png"), b"x".to_vec());
        files.insert(format!("{MEDIA}screenshots/../x.png.json"), b"{}".to_vec());
        files.insert(format!("{CHATS}../evil.bin"), b"x".to_vec());
        home.parts().import(&files).unwrap();
        assert!(home.saves.files().is_empty());
        assert!(home.media.items().is_empty());
        assert!(!home.root.join("escape.sav").exists());
        assert!(home.online.chat_files().is_empty());
    }

    #[test]
    fn an_export_and_its_import_count_every_byte_up_to_their_totals() {
        let mut home = Home::new("counted");
        home.fill();
        let files = home.parts().export();
        let counter = Counter::default();
        counter.set_total(export_bytes(&files, home.media.items()));
        let mut packer = Packer::new(counter.clone());
        packer.add_all(&files);
        assert!(!counter.bytes().is_done());
        let (tx, rx) = std::sync::mpsc::channel();
        home.media.read_files(&spawner(), move |media| {
            let _ = tx.send(media);
        });
        while let Ok(Some((id, bytes))) = rx.recv() {
            packer.add_media(&id, &bytes);
        }
        let progress = counter.bytes();
        assert!(progress.is_done(), "{progress:?}");
        assert_eq!(Some(progress.done), progress.total);

        let zip = packer.finish().unwrap();
        let reading = Counter::default();
        let back = unpack_counted(zip.into(), &reading).unwrap();
        assert_eq!(back.len(), files.len() + 2);
        let read = reading.bytes();
        assert!(read.is_done(), "{read:?}");
    }

    #[test]
    fn an_import_reads_many_files_per_tick_within_its_budget() {
        let mut files = Files::new();
        for i in 0..200 {
            files.insert(format!("saves/game {i}.sav"), vec![i as u8; 64]);
        }
        let zip = pack(&files).unwrap();
        let counter = Counter::default();
        let mut slow = Unpacker::new(zip.clone().into(), counter.clone()).unwrap();
        assert!(slow.step_for(std::time::Duration::ZERO).unwrap());
        assert!(!counter.bytes().is_done());

        let whole = Counter::default();
        let mut fast = Unpacker::new(zip.into(), whole.clone()).unwrap();
        assert!(!fast.step_for(std::time::Duration::from_secs(60)).unwrap());
        assert!(whole.bytes().is_done());
        assert_eq!(fast.finish().len(), files.len());
    }

    fn spawner() -> citrine_gb_threads::Spawner {
        citrine_gb_threads::Threads::new(citrine_gb_threads::Mode::Threads).spawner()
    }
}

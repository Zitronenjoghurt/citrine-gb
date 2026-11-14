#[cfg(not(target_arch = "wasm32"))]
use crate::app::ui_state::recent::{RecentRom, RecentRoms};
use crate::app::ui_state::settings::Settings;
use crate::icons;
use crate::media::{MediaId, MediaInfo, MediaStore};
use crate::online::Online;
use crate::online::chat::history::{decode_history, encode_history, merge_history};
use crate::progress::Counter;
use crate::stats::{Book, GameRecord, Stats};
use crate::storage::SaveStore;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{self, Read, Write};

pub const FILE_EXTENSION: &str = "zip";
const MANIFEST: &str = "citrine-data.json";
const FORMAT: u64 = 1;
const MAX_FILE_BYTES: u64 = 1 << 30;
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
];

pub type Files = BTreeMap<String, Vec<u8>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kind {
    pub name: &'static str,
    pub icon: &'static str,
    pub units: (&'static str, &'static str),
    pub nouns: (&'static str, &'static str),
}

impl Kind {
    pub fn unit(&self, n: usize) -> String {
        match self.units {
            ("", "") => n.to_string(),
            units => counted(n, units),
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
    pub bytes: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Change {
    pub added: usize,
    pub replaced: usize,
}

pub trait Part {
    fn kind(&self) -> Kind;
    fn tally(&mut self) -> Tally;
    fn export(&mut self, out: &mut Files);
    fn plan(&mut self, files: &Files) -> Change;
    fn import(&mut self, files: &Files) -> io::Result<()>;
    fn delete_all(&mut self) -> io::Result<()>;
}

pub struct Parts<'a> {
    pub saves: &'a SaveStore,
    pub media: &'a mut MediaStore,
    pub online: &'a mut Online,
    pub settings: &'a mut Settings,
    pub stats: &'a mut Stats,
    #[cfg(not(target_arch = "wasm32"))]
    pub recent: &'a mut RecentRoms,
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
        visit(&mut Saves(self.saves));
        visit(&mut Media(&mut *self.media));
        visit(&mut StatsPart(&mut *self.stats));
        visit(&mut Chats(&mut *self.online));
        visit(&mut SettingsPart(&mut *self.settings));
        #[cfg(not(target_arch = "wasm32"))]
        visit(&mut Recent(&mut *self.recent));
        visit(&mut GameNames(&mut *self.settings));
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
    }

    pub fn tallies(&mut self) -> Vec<(Kind, Tally)> {
        let mut rows = Vec::new();
        self.each(|part| rows.push((part.kind(), part.tally())));
        rows
    }

    pub fn export(&mut self) -> Files {
        let mut files = Files::new();
        self.each(|part| part.export(&mut files));
        files
    }

    pub fn plan(&mut self, files: &Files) -> Vec<(Kind, Change)> {
        let mut changes = Vec::new();
        self.each(|part| changes.push((part.kind(), part.plan(files))));
        changes
    }

    pub fn import(&mut self, files: &Files) -> io::Result<()> {
        let mut result = Ok(());
        self.each(|part| {
            if let Err(err) = part.import(files) {
                log::error!("Could not import the {}: {err}", part.kind().name);
                result = Err(err);
            }
        });
        result
    }

    pub fn delete_all(&mut self) -> io::Result<()> {
        let mut result = Ok(());
        self.each(|part| {
            if let Err(err) = part.delete_all() {
                log::error!("Could not delete the {}: {err}", part.kind().name);
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
            let mut file = zip.by_name(MANIFEST).map_err(|_| ReadError::NotOurs)?;
            let mut raw = Vec::new();
            file.read_to_end(&mut raw).map_err(|_| ReadError::NotOurs)?;
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
        self.total = self.total.saturating_add(file.size());
        if self.total > MAX_FILE_BYTES {
            return Err(ReadError::TooLarge);
        }
        let mut raw = Vec::with_capacity(file.size() as usize);
        file.read_to_end(&mut raw).map_err(|_| ReadError::NotZip)?;
        self.counter.add(raw.len() as u64);
        self.files.insert(name, raw);
        Ok(true)
    }

    pub fn finish(self) -> Files {
        self.files
    }
}

pub fn file_name(now: u64) -> String {
    format!(
        "citrine-data-{}.{FILE_EXTENSION}",
        crate::media::local_stamp(now, "%Y-%m-%d")
    )
}

struct Saves<'a>(&'a SaveStore);

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

fn file_items(files: &Files) -> BTreeMap<SaveItem, Vec<(&str, &[u8])>> {
    let mut items: BTreeMap<SaveItem, Vec<(&str, &[u8])>> = BTreeMap::new();
    for (path, bytes) in files.range(SAVES.to_owned()..) {
        let Some(rest) = path.strip_prefix(SAVES) else {
            break;
        };
        let Some((rom, name)) = rest.split_once('/') else {
            continue;
        };
        if !crate::storage::is_save_key(rom) || !crate::storage::is_user_file(name) {
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
    fn local_items(&self) -> BTreeMap<SaveItem, Vec<String>> {
        let mut items: BTreeMap<SaveItem, Vec<String>> = BTreeMap::new();
        for (rom, name) in self.0.files() {
            items.entry(save_item(&rom, &name)).or_default().push(name);
        }
        items
    }

    fn local_saved_at(&self, (rom, stem): &SaveItem) -> u64 {
        saved_at(self.0.read_file(rom, &format!("{stem}.meta")).as_deref())
    }

    fn incoming<'f>(&self, files: &'f Files) -> Vec<Incoming<'f>> {
        let mut local = self.local_items();
        file_items(files)
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
    fn kind(&self) -> Kind {
        Kind {
            name: "Saves",
            icon: icons::FLOPPY_DISK,
            units: ("", ""),
            nouns: ("save", "saves"),
        }
    }

    fn tally(&mut self) -> Tally {
        let files = self.0.files();
        Tally {
            count: self.local_items().len(),
            bytes: files
                .iter()
                .map(|(rom, name)| self.0.file_size(rom, name))
                .sum(),
        }
    }

    fn export(&mut self, out: &mut Files) {
        for (rom, name) in self.0.files() {
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
        self.0.delete_every_game()
    }
}

struct Media<'a>(&'a mut MediaStore);

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
    fn kind(&self) -> Kind {
        Kind {
            name: "Media",
            icon: icons::IMAGES,
            units: ("", ""),
            nouns: ("media file", "media files"),
        }
    }

    fn tally(&mut self) -> Tally {
        self.0.refresh();
        let items = self.0.items();
        Tally {
            count: items.len(),
            bytes: items.iter().map(|item| item.info.bytes).sum(),
        }
    }

    fn export(&mut self, out: &mut Files) {
        for item in self.0.items() {
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
    fn kind(&self) -> Kind {
        Kind {
            name: "Chats",
            icon: icons::CHAT_CIRCLE,
            units: ("message", "messages"),
            nouns: ("chat message", "chat messages"),
        }
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
            bytes: files.iter().map(|(_, raw)| raw.len() as u64).sum(),
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
    fn kind(&self) -> Kind {
        Kind {
            name: "Settings",
            icon: icons::GEAR,
            units: ("changed", "changed"),
            nouns: ("setting", "settings"),
        }
    }

    fn tally(&mut self) -> Tally {
        let mine = settings_json(self.0);
        let defaults = settings_json(&Settings::default());
        Tally {
            count: differences(&mine, &defaults),
            bytes: serde_json::to_vec(&mine).map_or(0, |raw| raw.len() as u64),
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
        let mut merged = settings_json(self.0);
        merged.extend(theirs);
        let Ok(adopted) = serde_json::from_value::<Settings>(Value::Object(merged)) else {
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
        let Ok(Value::Object(mut merged)) = serde_json::to_value(&*self.0) else {
            return Err(io::Error::other("settings unreadable"));
        };
        merged.extend(theirs);
        let mut adopted: Settings =
            serde_json::from_value(Value::Object(merged)).map_err(io::Error::other)?;
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
    fn kind(&self) -> Kind {
        Kind {
            name: "Game names",
            icon: icons::TAG,
            units: ("", ""),
            nouns: ("game name", "game names"),
        }
    }

    fn tally(&mut self) -> Tally {
        let names: BTreeMap<&str, &str> = self.0.game_names.iter().collect();
        Tally {
            count: names.len(),
            bytes: serde_json::to_vec(&names).map_or(0, |raw| raw.len() as u64),
        }
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
    fn kind(&self) -> Kind {
        Kind {
            name: "Stats",
            icon: icons::CHART_BAR,
            units: ("game", "games"),
            nouns: ("game's play time", "games' play time"),
        }
    }

    fn tally(&mut self) -> Tally {
        let book = self.0.book();
        if book.is_empty() {
            return Tally::default();
        }
        Tally {
            count: Self::games(book).len(),
            bytes: serde_json::to_vec(&Self::portable(book)).map_or(0, |raw| raw.len() as u64),
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
impl Recent<'_> {
    fn theirs(files: &Files) -> Vec<RecentRom> {
        files
            .get(RECENT)
            .and_then(|raw| serde_json::from_slice(raw).ok())
            .unwrap_or_default()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Part for Recent<'_> {
    fn kind(&self) -> Kind {
        Kind {
            name: "Recent games",
            icon: icons::CLOCK_USER,
            units: ("", ""),
            nouns: ("recent game", "recent games"),
        }
    }

    fn tally(&mut self) -> Tally {
        Tally {
            count: self.0.entries().len(),
            bytes: serde_json::to_vec(self.0.entries()).map_or(0, |raw| raw.len() as u64),
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

#[cfg(feature = "nds")]
pub mod ds_files;
#[cfg(ds_wifi)]
pub mod ds_wifi;
#[cfg(feature = "gba")]
pub mod gba_bios;
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
        online: Online,
        settings: Settings,
        stats: Stats,
        recent: RecentRoms,
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
            let root = std::env::temp_dir()
                .join(format!("citrine-your-data-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            let mut online = Online::default();
            online.persist_chats_in(root.join("data"));
            Self {
                saves: SaveStore::at(root.join("saves")),
                media: MediaStore::at(root.join("media")),
                online,
                settings: Settings::default(),
                stats: Stats::open(crate::stats::StatsStore::in_dir(root.join("stats"))),
                recent: RecentRoms::default(),
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
                online: &mut self.online,
                settings: &mut self.settings,
                stats: &mut self.stats,
                recent: &mut self.recent,
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
            });
        }
        History::from([(OnlineId::new(7).unwrap(), chat)])
    }

    fn counts(home: &mut Home) -> Vec<(&'static str, usize)> {
        home.parts()
            .tallies()
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
        assert_eq!(
            counted,
            [
                ("Saves", 4),
                ("Media", 2),
                ("Stats", 1),
                ("Chats", 2),
                ("Settings", 2),
                ("Recent games", 1),
                ("Game names", 1),
                #[cfg(feature = "gba")]
                ("GBA BIOS", 1),
                #[cfg(feature = "nds")]
                ("DS ARM9 BIOS", 1),
                #[cfg(feature = "nds")]
                ("DS ARM7 BIOS", 1),
                #[cfg(feature = "nds")]
                ("DS firmware", 1),
                #[cfg(feature = "nds")]
                ("Pokéwalker", 1),
                #[cfg(ds_wifi)]
                ("DS wifi connection", 1),
            ]
        );

        let file = through_zip(&before);
        assert_eq!(file, before, "the zip holds every file as it was");
        home.parts().delete_all().unwrap();
        assert!(
            counts(&mut home).iter().all(|(_, count)| *count == 0),
            "{:?}",
            counts(&mut home)
        );
        assert!(home.settings.online.joined.is_some(), "the account stays");
        #[cfg(feature = "nds")]
        for file in <crate::app::ds_firmware::DsFile as strum::IntoEnumIterator>::iter() {
            assert_eq!(home.bios.0.ds_file(file), None, "{file:?} stayed");
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
                .ds_file(crate::app::ds_firmware::DsFile::Firmware),
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
        assert_eq!(
            of("Saves"),
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

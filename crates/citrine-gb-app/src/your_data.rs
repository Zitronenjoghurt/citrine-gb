#[cfg(not(target_arch = "wasm32"))]
use crate::app::ui_state::recent::{RecentRom, RecentRoms};
use crate::app::ui_state::settings::Settings;
use crate::icons;
use crate::media::{MediaId, MediaInfo, MediaStore};
use crate::online::Online;
use crate::online::chat::history::{decode_history, encode_history, merge_history};
use crate::stats::{Book, GameRecord, Stats};
use crate::storage::SaveStore;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{self, Read, Write};

pub const FILE_EXTENSION: &str = "zip";
const MANIFEST: &str = "citrine-data.json";
const FORMAT: u64 = 1;
const MAX_FILE_BYTES: u64 = 1 << 30;

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

const KEPT_SETTINGS: &[&str] = &["online", "synced", "current_tab", "game_names"];

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

pub fn add_media_files(files: &mut Files, media: Vec<(MediaId, Vec<u8>)>) {
    for (id, bytes) in media {
        files.insert(format!("{MEDIA}{}", id.as_str()), bytes);
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

pub fn pack(files: &Files) -> io::Result<Vec<u8>> {
    let mut zip = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
    let manifest = serde_json::json!({
        "format": FORMAT,
        "made_by": format!("Citrine {}", env!("CARGO_PKG_VERSION")),
        "made_at": crate::storage::now_unix(),
    });
    let packed = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);
    let stored = packed.compression_method(zip::CompressionMethod::Stored);
    zip.start_file(MANIFEST, packed).map_err(io::Error::other)?;
    zip.write_all(&serde_json::to_vec_pretty(&manifest).map_err(io::Error::other)?)?;
    for (path, bytes) in files {
        let options = if already_packed(path) { stored } else { packed };
        zip.start_file(path.as_str(), options)
            .map_err(io::Error::other)?;
        zip.write_all(bytes)?;
    }
    Ok(zip.finish().map_err(io::Error::other)?.into_inner())
}

fn already_packed(path: &str) -> bool {
    [".png", ".mp4", ".snap", ".thumb", ".bin"]
        .iter()
        .any(|ext| path.ends_with(ext))
}

pub fn unpack(bytes: &[u8]) -> Result<Files, ReadError> {
    let mut zip = zip::ZipArchive::new(io::Cursor::new(bytes)).map_err(|_| ReadError::NotZip)?;
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
    let mut files = Files::new();
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|_| ReadError::NotZip)?;
        let name = file.name().map_err(|_| ReadError::NotZip)?.into_owned();
        if file.is_dir() || name == MANIFEST {
            continue;
        }
        total = total.saturating_add(file.size());
        if total > MAX_FILE_BYTES {
            return Err(ReadError::TooLarge);
        }
        let mut raw = Vec::with_capacity(file.size() as usize);
        file.read_to_end(&mut raw).map_err(|_| ReadError::NotZip)?;
        files.insert(name, raw);
    }
    Ok(files)
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
        if !crate::storage::cloud::is_game_key(rom) || !crate::storage::is_user_file(name) {
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;

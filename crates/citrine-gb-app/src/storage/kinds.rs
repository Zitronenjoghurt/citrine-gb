use crate::icons;
use std::collections::BTreeMap;
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

pub const EXPORT_LEAVES_COPIES: &str = "Export leaves out game copies.";
const COPIES_HINT: &str = "Kept so Recent games can start them. Export leaves out game copies.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, EnumIter)]
pub enum Section {
    Data,
    Cache,
}

impl Section {
    pub fn title(self) -> &'static str {
        match self {
            Self::Data => "Your data",
            Self::Cache => "Cache",
        }
    }

    pub fn kinds(self) -> impl Iterator<Item = Kind> {
        Kind::iter().filter(move |kind| kind.section() == self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, EnumIter)]
pub enum Kind {
    Saves,
    States,
    Screenshots,
    Recordings,
    Settings,
    GameNames,
    RecentGames,
    SystemFiles,
    DsWifi,
    Stats,
    Chats,
    Pokewalker,
    GameCopies,
    OtherData,
    Covers,
    GameInfo,
    AchievementBadges,
    GameListIndex,
    CompiledCode,
    VideoCodec,
    GameArchive,
    Temporary,
    OtherCache,
}

impl Kind {
    pub fn section(self) -> Section {
        match self {
            Self::Saves
            | Self::States
            | Self::Screenshots
            | Self::Recordings
            | Self::Settings
            | Self::GameNames
            | Self::RecentGames
            | Self::SystemFiles
            | Self::DsWifi
            | Self::Stats
            | Self::Chats
            | Self::Pokewalker
            | Self::GameCopies
            | Self::OtherData => Section::Data,
            Self::Covers
            | Self::GameInfo
            | Self::AchievementBadges
            | Self::GameListIndex
            | Self::CompiledCode
            | Self::VideoCodec
            | Self::GameArchive
            | Self::Temporary
            | Self::OtherCache => Section::Cache,
        }
    }

    pub fn counted(self, n: usize) -> Option<String> {
        let (one, many) = match self {
            Self::Saves | Self::Pokewalker => ("save", "saves"),
            Self::States => ("save state", "save states"),
            Self::Screenshots => ("screenshot", "screenshots"),
            Self::Recordings => ("recording", "recordings"),
            Self::Settings => ("changed", "changed"),
            Self::GameNames => ("name", "names"),
            Self::RecentGames => ("game", "games"),
            Self::SystemFiles | Self::OtherData => ("file", "files"),
            Self::DsWifi => ("connection", "connections"),
            Self::Stats => ("game played", "games played"),
            Self::Chats => ("message", "messages"),
            Self::GameCopies => ("copy", "copies"),
            _ => return None,
        };
        (n > 0).then(|| format!("{n} {}", if n == 1 { one } else { many }))
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Saves => "Saves",
            Self::States => "Save states",
            Self::Screenshots => "Screenshots",
            Self::Recordings => "Recordings",
            Self::Settings => "Settings",
            Self::GameNames => "Game names",
            Self::RecentGames => "Recent games",
            Self::SystemFiles => "BIOS and boot ROMs",
            Self::DsWifi => "DS wifi connection",
            Self::Stats => "Stats",
            Self::Chats => "Chats",
            Self::Pokewalker => "Pokéwalker",
            Self::GameCopies => "Game copies",
            Self::OtherData => "Other data files",
            Self::OtherCache => "Other cache files",
            Self::Covers => "Game covers",
            Self::GameInfo => "Game info",
            Self::AchievementBadges => "Achievement badges",
            Self::GameListIndex => "Game list index",
            Self::CompiledCode => "Compiled game code",
            Self::VideoCodec => "Video codec",
            Self::GameArchive => "Game Archive",
            Self::Temporary => "Temporary files",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Saves => icons::FLOPPY_DISK,
            Self::States => icons::STACK,
            Self::Screenshots => icons::CAMERA,
            Self::Recordings => icons::VIDEO_CAMERA,
            Self::Settings => icons::GEAR,
            Self::GameNames => icons::TAG,
            Self::RecentGames => icons::CLOCK_COUNTER_CLOCKWISE,
            Self::SystemFiles => icons::CPU,
            Self::DsWifi => icons::WIFI_HIGH,
            Self::Stats => icons::CHART_BAR,
            Self::Chats => icons::CHAT_CIRCLE,
            Self::Pokewalker => icons::POKEWALKER,
            Self::GameCopies => icons::GAME_CONTROLLER,
            Self::OtherData => icons::FILE,
            Self::Covers => icons::IMAGE_SQUARE,
            Self::GameInfo => icons::IDENTIFICATION_CARD,
            Self::AchievementBadges => icons::TROPHY,
            Self::GameListIndex => icons::LIST_MAGNIFYING_GLASS,
            Self::CompiledCode => icons::LIGHTNING,
            Self::VideoCodec => icons::FILE_VIDEO,
            Self::GameArchive => icons::CLOUD_ARROW_DOWN,
            Self::Temporary => icons::HOURGLASS,
            Self::OtherCache => icons::FILE_DASHED,
        }
    }

    pub fn hint(self) -> Option<&'static str> {
        match self {
            Self::CompiledCode => Some("Each game starts slower once, while it compiles again."),
            Self::VideoCodec => Some("Downloads again the next time you record."),
            Self::GameListIndex => Some("The library reads each game file again."),
            Self::GameCopies => Some(COPIES_HINT),
            Self::GameArchive => Some(
                "Games you downloaded from the Game Archive. They download again when you play them.",
            ),
            Self::Temporary => Some(
                "Downloaded updates, crash reports and files a recording or export leaves behind.",
            ),
            _ => None,
        }
    }

    pub fn listed(self, bytes: u64) -> bool {
        self.shown() && (bytes > 0 || !matches!(self, Self::OtherData | Self::OtherCache))
    }

    pub fn in_sentence(self) -> String {
        match self {
            Self::GameArchive => "Game Archive downloads".to_owned(),
            Self::Pokewalker | Self::DsWifi => self.name().to_owned(),
            _ => {
                let name = self.name();
                let mut chars = name.chars();
                chars.next().map_or_else(String::new, |first| {
                    first.to_lowercase().chain(chars).collect()
                })
            }
        }
    }

    pub fn shown(self) -> bool {
        match self {
            Self::CompiledCode => jit_available(),
            Self::VideoCodec => cfg!(all(not(target_arch = "wasm32"), not(target_os = "android"))),
            Self::Pokewalker => cfg!(feature = "nds"),
            Self::RecentGames => cfg!(not(target_arch = "wasm32")),
            Self::DsWifi => cfg!(ds_wifi),
            _ => true,
        }
    }
}

#[cfg(feature = "nds")]
fn jit_available() -> bool {
    citrine_gb::nds::machine::JIT_AVAILABLE
}

#[cfg(not(feature = "nds"))]
fn jit_available() -> bool {
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, EnumIter)]
pub enum Root {
    Data,
    Cache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, EnumIter)]
pub enum Folder {
    Saves,
    Settings,
    Media,
    Stats,
    Walker,
    Chats,
    RomCopies,
    AchievementsToken,
    Covers,
    CoverNames,
    GameFacts,
    GamePages,
    SpeciesNames,
    Badges,
    AchievementLists,
    RomIds,
    Jit,
    Codecs,
    MediaTemp,
    Updates,
    Export,
    Reports,
    GameArchive,
}

impl Folder {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Saves => "saves",
            Self::Settings => "settings",
            Self::Media => "media",
            Self::Stats => "stats",
            Self::Walker => "walker",
            Self::Chats => "chats",
            Self::RomCopies => "roms",
            Self::AchievementsToken => "retroachievements-token",
            Self::Covers => "covers",
            Self::CoverNames => "cover-names",
            Self::GameFacts => "game-facts",
            Self::GamePages => "game-pages",
            Self::SpeciesNames => "species-names",
            Self::Badges => "badges",
            Self::AchievementLists => "achievements",
            Self::RomIds => "rom-ids",
            Self::Jit => "jit",
            Self::Codecs => "codecs",
            Self::MediaTemp => "media-temp",
            Self::Updates => "updates",
            Self::Export => "export",
            Self::Reports => "reports",
            Self::GameArchive => "game-archive",
        }
    }

    pub fn root(self) -> Root {
        match self.kind_of("") {
            kind if kind.section() == Section::Cache => Root::Cache,
            _ => Root::Data,
        }
    }

    pub fn kind_of(self, rest: &str) -> Kind {
        match self {
            Self::Saves => saves_kind(rest),
            Self::Settings => settings_kind(rest),
            Self::Media => media_kind(rest),
            Self::Stats => Kind::Stats,
            Self::Walker => Kind::Pokewalker,
            Self::Chats => Kind::Chats,
            Self::RomCopies => Kind::GameCopies,
            Self::AchievementsToken => Kind::Settings,
            Self::Covers | Self::CoverNames => Kind::Covers,
            Self::GameFacts | Self::GamePages | Self::SpeciesNames => Kind::GameInfo,
            Self::Badges | Self::AchievementLists => Kind::AchievementBadges,
            Self::RomIds => Kind::GameListIndex,
            Self::Jit => Kind::CompiledCode,
            Self::Codecs => Kind::VideoCodec,
            Self::GameArchive => Kind::GameArchive,
            Self::MediaTemp | Self::Updates | Self::Export | Self::Reports => Kind::Temporary,
        }
    }

    pub fn of(kind: Kind) -> impl Iterator<Item = Folder> {
        Folder::iter()
            .filter(move |folder| folder.root() == Root::Cache && folder.kind_of("") == kind)
    }
}

fn saves_kind(rest: &str) -> Kind {
    let file = rest.rsplit('/').next().unwrap_or(rest);
    if file.starts_with("slot") || file.starts_with("auto") {
        Kind::States
    } else {
        Kind::Saves
    }
}

fn settings_kind(rest: &str) -> Kind {
    if rest.starts_with("gba-bios")
        || rest.starts_with("gb-boot-rom")
        || rest.starts_with("gbc-boot-rom")
        || rest.starts_with("nds-arm")
        || rest.starts_with("nds-firmware")
    {
        Kind::SystemFiles
    } else if rest.starts_with("nds-wifi") {
        Kind::DsWifi
    } else {
        Kind::Settings
    }
}

fn media_kind(rest: &str) -> Kind {
    if rest.split('/').any(|part| part == "recordings") {
        Kind::Recordings
    } else {
        Kind::Screenshots
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, EnumIter)]
pub enum RootFile {
    AppState,
    JitKey,
    JitPart,
    RustCrash,
    JavaCrash,
}

impl RootFile {
    pub const fn name(self) -> &'static str {
        match self {
            Self::AppState => "app.ron",
            Self::JitKey => "jit.key",
            Self::JitPart => "jit.part",
            Self::RustCrash => "crash-rust.txt",
            Self::JavaCrash => "crash-java.txt",
        }
    }

    pub fn root(self) -> Root {
        match self.kind() {
            kind if kind.section() == Section::Cache => Root::Cache,
            _ => Root::Data,
        }
    }

    pub fn kind(self) -> Kind {
        match self {
            Self::AppState | Self::JitKey | Self::JitPart => Kind::Settings,
            Self::RustCrash | Self::JavaCrash => Kind::Temporary,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn of(kind: Kind) -> impl Iterator<Item = RootFile> {
        RootFile::iter().filter(move |file| file.root() == Root::Cache && file.kind() == kind)
    }
}

pub fn claim(root: Root, path: &str) -> Kind {
    let (first, rest) = path.split_once('/').unwrap_or((path, ""));
    if rest.is_empty()
        && let Some(file) =
            RootFile::iter().find(|file| file.root() == root && file.name() == first)
    {
        return file.kind();
    }
    Folder::iter()
        .find(|folder| folder.name() == first && folder.root() == root && !rest.is_empty())
        .map(|folder| folder.kind_of(rest))
        .unwrap_or(match root {
            Root::Data => Kind::OtherData,
            Root::Cache => Kind::OtherCache,
        })
}

pub type Usage = BTreeMap<Kind, u64>;

pub fn listed(section: Section, usage: &Usage) -> impl Iterator<Item = Kind> + '_ {
    section
        .kinds()
        .filter(|kind| kind.listed(usage.get(kind).copied().unwrap_or(0)))
}

pub fn carve_from_settings(usage: &mut Usage, kind: Kind, bytes: u64) {
    let settings = usage.entry(Kind::Settings).or_default();
    let bytes = bytes.min(*settings);
    *settings -= bytes;
    if bytes > 0 {
        *usage.entry(kind).or_default() += bytes;
    }
}

pub mod other_data {
    use super::{Kind, Root, claim};

    pub fn is_other(path: &str) -> bool {
        let safe = !path.is_empty()
            && path.split('/').all(|part| {
                !part.is_empty() && part != "." && part != ".." && !part.contains('\\')
            });
        safe && claim(Root::Data, path) == Kind::OtherData
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn list() -> Vec<String> {
        let mut found = Vec::new();
        super::walk(&crate::storage::data_dir(), "", &mut |path, _| {
            if is_other(path) {
                found.push(path.to_owned());
            }
        });
        found
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn read(path: &str) -> Option<Vec<u8>> {
        is_other(path)
            .then(|| std::fs::read(crate::storage::data_dir().join(path)).ok())
            .flatten()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn write(path: &str, data: &[u8]) -> std::io::Result<()> {
        if !is_other(path) {
            return Err(std::io::Error::other("not an unclaimed data file"));
        }
        let file = crate::storage::data_dir().join(path);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(file, data)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn delete(path: &str) -> std::io::Result<()> {
        if !is_other(path) {
            return Ok(());
        }
        match std::fs::remove_file(crate::storage::data_dir().join(path)) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err),
            _ => Ok(()),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn split(path: &str) -> Option<(&str, &str)> {
        is_other(path).then(|| path.split_once('/')).flatten()
    }

    #[cfg(target_arch = "wasm32")]
    pub fn list() -> Vec<String> {
        let settings = crate::storage::Backend::settings();
        settings
            .namespaces()
            .into_iter()
            .flat_map(|namespace| {
                settings
                    .list(&namespace)
                    .into_iter()
                    .map(move |name| format!("{namespace}/{name}"))
            })
            .filter(|path| is_other(path))
            .collect()
    }

    #[cfg(target_arch = "wasm32")]
    pub fn read(path: &str) -> Option<Vec<u8>> {
        let (namespace, name) = split(path)?;
        crate::storage::Backend::settings().read(namespace, name)
    }

    #[cfg(target_arch = "wasm32")]
    pub fn write(path: &str, data: &[u8]) -> std::io::Result<()> {
        let (namespace, name) =
            split(path).ok_or_else(|| std::io::Error::other("not an unclaimed data file"))?;
        crate::storage::Backend::settings().write(namespace, name, data)
    }

    #[cfg(target_arch = "wasm32")]
    pub fn delete(path: &str) -> std::io::Result<()> {
        match split(path) {
            Some((namespace, name)) => crate::storage::Backend::settings().delete(namespace, name),
            None => Ok(()),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn usage() -> Usage {
    let mut usage = Usage::new();
    for (root, dir) in [
        (Root::Data, super::data_dir()),
        (Root::Cache, super::cache_dir()),
    ] {
        walk(&dir, "", &mut |path, bytes| {
            *usage.entry(claim(root, path)).or_default() += bytes;
        });
    }
    usage
}

#[cfg(not(target_arch = "wasm32"))]
pub fn count_folder(usage: &mut Usage, kind: Kind, dir: &std::path::Path) {
    walk(dir, "", &mut |_, bytes| {
        *usage.entry(kind).or_default() += bytes;
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn walk(dir: &std::path::Path, prefix: &str, visit: &mut dyn FnMut(&str, u64)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            walk(&entry.path(), &path, visit);
        } else if let Ok(meta) = entry.metadata() {
            visit(&path, meta.len());
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub fn usage() -> Usage {
    let mut usage = Usage::new();
    let cache = super::Backend::cache();
    for namespace in cache.namespaces() {
        for name in cache.list(&namespace) {
            let path = format!("{namespace}/{name}");
            *usage.entry(claim(Root::Cache, &path)).or_default() += cache.size(&namespace, &name);
        }
    }
    let settings = super::Backend::settings();
    for namespace in settings.namespaces() {
        for name in settings.list(&namespace) {
            let path = format!("{namespace}/{name}");
            *usage.entry(claim(Root::Data, &path)).or_default() += settings.size(&namespace, &name);
        }
    }
    let saves = super::Backend::default();
    for namespace in saves.namespaces() {
        for name in saves.list(&namespace) {
            let path = format!("{}/{namespace}/{name}", Folder::Saves.name());
            *usage.entry(claim(Root::Data, &path)).or_default() += saves.size(&namespace, &name);
        }
    }
    usage
}

#[cfg(not(target_arch = "wasm32"))]
pub fn size(kind: Kind) -> u64 {
    let cache = super::cache_dir();
    let mut total = 0;
    for folder in Folder::of(kind) {
        walk(&cache.join(folder.name()), "", &mut |_, bytes| {
            total += bytes
        });
    }
    for file in RootFile::of(kind) {
        total += std::fs::metadata(cache.join(file.name())).map_or(0, |meta| meta.len());
    }
    total
}

#[cfg(target_arch = "wasm32")]
pub fn size(kind: Kind) -> u64 {
    let folders = super::Folders::default();
    Folder::of(kind)
        .flat_map(|folder| {
            let folders = &folders;
            folders
                .list(folder)
                .into_iter()
                .map(move |name| folders.size(folder, &name))
        })
        .sum()
}

pub fn clear(kind: Kind) -> std::io::Result<()> {
    let folders = super::Folders::default();
    for folder in Folder::of(kind) {
        folders.clear(folder)?;
    }
    #[cfg(not(target_arch = "wasm32"))]
    for file in RootFile::of(kind) {
        match std::fs::remove_file(super::cache_dir().join(file.name())) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => return Err(err),
            _ => {}
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    if kind == Kind::OtherCache {
        clear_unclaimed(&super::cache_dir())?;
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn clear_unclaimed(dir: &std::path::Path) -> std::io::Result<()> {
    let mut unclaimed = Vec::new();
    walk(dir, "", &mut |path, _| {
        if claim(Root::Cache, path) == Kind::OtherCache {
            unclaimed.push(path.to_owned());
        }
    });
    for path in unclaimed {
        match std::fs::remove_file(dir.join(path)) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => return Err(err),
            _ => {}
        }
    }
    Ok(())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn every_folder_has_one_root_and_a_kind_of_its_section() {
        for folder in Folder::iter() {
            let kind = folder.kind_of("x");
            let section = match folder.root() {
                Root::Data => Section::Data,
                Root::Cache => Section::Cache,
            };
            assert_eq!(kind.section(), section, "{folder:?}");
        }
    }

    #[test]
    fn files_outside_known_folders_count_as_other_files() {
        assert_eq!(claim(Root::Data, "mystery/thing.bin"), Kind::OtherData);
        assert_eq!(claim(Root::Cache, "loose.bin"), Kind::OtherCache);
        assert_eq!(claim(Root::Cache, "covers"), Kind::OtherCache);
    }

    #[test]
    fn save_files_split_into_saves_and_states() {
        assert_eq!(claim(Root::Data, "saves/abc/battery.sav"), Kind::Saves);
        assert_eq!(claim(Root::Data, "saves/abc/slot3.snap"), Kind::States);
        assert_eq!(claim(Root::Data, "saves/abc/auto1.thumb"), Kind::States);
        assert_eq!(
            claim(Root::Data, "media/recordings/a.mp4"),
            Kind::Recordings
        );
        assert_eq!(
            claim(Root::Cache, "rom-ids/rom-ids.json"),
            Kind::GameListIndex
        );
        assert_eq!(claim(Root::Data, "app.ron"), Kind::Settings);
        assert_eq!(claim(Root::Cache, "jit/abc.jit"), Kind::CompiledCode);
    }

    #[test]
    fn every_kind_has_its_own_icon() {
        let mut seen = BTreeMap::new();
        for kind in Kind::iter() {
            if let Some(other) = seen.insert(kind.icon(), kind) {
                panic!("{kind:?} and {other:?} share an icon");
            }
        }
    }

    #[test]
    fn names_read_inside_a_sentence() {
        assert_eq!(Kind::CompiledCode.in_sentence(), "compiled game code");
        assert_eq!(Kind::GameArchive.in_sentence(), "Game Archive downloads");
    }

    #[test]
    fn size_of_one_kind_counts_only_its_files() {
        let _dirs = crate::storage::TestDirs::new("kinds-size");
        let cache = crate::storage::cache_dir();
        std::fs::create_dir_all(cache.join("jit")).unwrap();
        std::fs::create_dir_all(cache.join("covers")).unwrap();
        std::fs::write(cache.join("jit/a.jit"), [0; 300]).unwrap();
        std::fs::write(cache.join("covers/a.png"), [0; 50]).unwrap();
        assert_eq!(size(Kind::CompiledCode), 300);
        assert_eq!(usage().get(&Kind::Covers), Some(&50));
    }

    #[test]
    fn the_copies_hint_and_the_delete_ask_say_the_export_part_alike() {
        assert!(COPIES_HINT.ends_with(EXPORT_LEAVES_COPIES));
        assert_eq!(Kind::GameCopies.hint(), Some(COPIES_HINT));
    }
}

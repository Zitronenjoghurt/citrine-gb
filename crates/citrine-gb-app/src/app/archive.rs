use super::Citrine;
use crate::app::ui_state::library::{ArchiveCopy, ArchiveListing, LibraryGame};
use crate::app::ui_state::recent::copies::RomCopies;
use crate::online::Online;
use crate::online::archive::{ArchiveDownload, ArchiveFailure};
use crate::utils::file_loader::PickedFile;
use citrine_gb::console::ConsoleKind;
use citrine_gb_net::{ArchiveConsole, ArchiveGame, ArchiveId, UnpackError};
use citrine_gb_threads::Pending;
use std::path::Path;

const DOWNLOAD_REPAINT: std::time::Duration = std::time::Duration::from_millis(100);

type Unpacked = Result<Vec<u8>, UnpackError>;

pub struct ArchiveShelf {
    store: RomCopies,
    unpacking: Vec<(ArchiveGame, String, Pending<Unpacked>)>,
    play_after: Option<String>,
    online: Option<bool>,
    known: bool,
}

impl Default for ArchiveShelf {
    fn default() -> Self {
        Self::with_store(RomCopies::archive())
    }
}

impl ArchiveShelf {
    pub fn with_store(store: RomCopies) -> Self {
        Self {
            store,
            unpacking: Vec::new(),
            play_after: None,
            online: None,
            known: false,
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn store(&self) -> &RomCopies {
        &self.store
    }

    pub fn is_unpacking(&self, path: &str) -> bool {
        self.unpacking.iter().any(|(_, at, _)| at == path)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

fn console_of(console: ArchiveConsole) -> Option<ConsoleKind> {
    match console {
        ArchiveConsole::GameBoy => Some(ConsoleKind::Gb),
        ArchiveConsole::GameBoyColor => Some(ConsoleKind::Gbc),
        ArchiveConsole::GameBoyAdvance => Some(ConsoleKind::Gba),
        ArchiveConsole::Ds => Some(ConsoleKind::Nds),
        ArchiveConsole::Other => None,
    }
}

fn listing(store: &RomCopies, game: &ArchiveGame) -> ArchiveListing {
    let rom_key = hex(&game.sha256);
    let file_name = game.file_name();
    let path = store.path_for(&rom_key, &file_name);
    ArchiveListing {
        name: crate::game_name::game_name(&game.sha1, Some(&file_name), ""),
        console: console_of(game.console),
        rom_key,
        sha1: hex(&game.sha1),
        crc32: game.crc32,
        copy: ArchiveCopy {
            id: game.id.to_string(),
            download: game.download,
            size: game.size,
            downloaded: store.holds(&path) == Some(true),
        },
        path: path.to_string_lossy().into_owned(),
    }
}

pub(crate) fn download_progress(online: &Online, game: &LibraryGame) -> Option<(u64, u64)> {
    online.archive_progress(ArchiveId::parse(&game.archive.as_ref()?.id)?)
}

fn failure_words(name: &str, failure: ArchiveFailure) -> String {
    match failure {
        ArchiveFailure::Damaged => format!("Couldn't download {name}: the download was damaged"),
        ArchiveFailure::Refused(_) => format!("Couldn't download {name}"),
    }
}

impl Citrine {
    pub(super) fn follow_archive(&mut self, ctx: &egui::Context) {
        let owner = self.ui.settings.online.cloud_owner();
        if self
            .ui
            .library
            .archive_owner()
            .is_some_and(|had| Some(had) != owner.as_deref())
        {
            self.drop_archive();
        }
        if let Some(games) = self.online.take_archive_list()
            && let Some(owner) = owner
        {
            let store = &self.archive.store;
            let listings: Vec<ArchiveListing> =
                games.iter().map(|game| listing(store, game)).collect();
            let now = crate::storage::now_unix();
            let gone = self.ui.library.apply_archive(&owner, listings, now);
            self.forget_archive_games(&gone);
            if self.ui.settings.covers {
                self.want_archive_covers(&games, now);
            }
            self.archive.known = false;
            self.archive.online = None;
        }
        if !self.archive.known {
            self.check_archive_copies();
        }
        let online = self.online.is_connected();
        if self.archive.online != Some(online) {
            self.archive.online = Some(online);
            self.ui.library.mark_archive(online);
        }
        for download in self.online.take_archive_done() {
            self.unpack_archive(download);
        }
        for (id, failure) in self.online.take_archive_failed() {
            self.archive_failed(id, failure);
        }
        self.finish_unpacking();
        let busy = !self.archive.unpacking.is_empty()
            || self
                .online
                .archive_games()
                .iter()
                .any(|game| self.online.archive_progress(game.id).is_some());
        if busy {
            ctx.request_repaint_after(DOWNLOAD_REPAINT);
        }
    }

    fn want_archive_covers(&mut self, games: &[ArchiveGame], now: u64) {
        let store = self.emulator.store.covers();
        for game in games {
            let Some(console) = console_of(game.console) else {
                continue;
            };
            let wanted = crate::covers::Game::of(&hex(&game.sha256), console, game.sha1, &[])
                .named(&game.file_name());
            self.covers.want(&store, wanted, now);
        }
    }

    fn check_archive_copies(&mut self) {
        let store = &self.archive.store;
        let pending = self.ui.games.pending_download();
        let mut found = Vec::new();
        for game in self.ui.library.archive_games() {
            match store.holds(Path::new(&game.path)) {
                Some(has) => found.push((
                    game.path.clone(),
                    has && pending != Some(game.path.as_str()),
                )),
                None => return,
            }
        }
        for (path, has) in found {
            self.ui.library.set_downloaded(&path, has);
        }
        self.archive.known = true;
        self.archive.online = None;
    }

    fn drop_archive(&mut self) {
        let games = self.ui.library.drop_archive();
        self.forget_archive_games(&games);
        self.archive.store.clear();
    }

    pub(super) fn clear_archive_downloads(&mut self) {
        self.archive.store.clear();
        self.archive.known = false;
    }

    fn forget_archive_games(&mut self, games: &[LibraryGame]) {
        for game in games {
            let path = Path::new(&game.path);
            self.ui.recent.remove(path);
            self.archive.store.drop_copies([path]);
            if let Some(id) = game.archive.as_ref().and_then(|a| ArchiveId::parse(&a.id)) {
                self.online.archive_cancel(id);
            }
        }
        let gone = |path: &str| games.iter().any(|game| game.path == path);
        self.archive.unpacking.retain(|(_, path, _)| !gone(path));
        if self.archive.play_after.as_deref().is_some_and(gone) {
            self.archive.play_after = None;
        }
    }

    pub(super) fn archive_id(&self, path: &str) -> Option<ArchiveId> {
        let game = self.ui.library.find(path)?;
        ArchiveId::parse(&game.archive.as_ref()?.id)
    }

    pub(super) fn play_archive(&mut self, path: &str) -> bool {
        let Some(game) = self.ui.library.find(path) else {
            return false;
        };
        let Some(copy) = game.archive.as_ref() else {
            return false;
        };
        if copy.downloaded {
            return false;
        }
        if self.ui.games.pending_download() == Some(path) {
            self.ui.games.undo = None;
            self.keep_download(path);
            return false;
        }
        let name = game.name.clone();
        let id = ArchiveId::parse(&copy.id);
        self.try_start_audio();
        self.archive.play_after = Some(path.to_owned());
        if self.archive.is_unpacking(path) {
            return true;
        }
        if !id.is_some_and(|id| self.online.archive_fetch(id)) {
            self.archive.play_after = None;
            self.toasts.error(format!("Couldn't download {name}"));
        }
        true
    }

    pub(super) fn cancel_archive(&mut self, path: &str) {
        if let Some(id) = self.archive_id(path) {
            self.online.archive_cancel(id);
        }
        if self.archive.play_after.as_deref() == Some(path) {
            self.archive.play_after = None;
        }
    }

    pub(super) fn delete_archive_download(&mut self, path: &str) {
        let Some(game) = self.ui.library.find(path) else {
            return;
        };
        if game.archive.is_none() {
            return;
        }
        let name = game.name.clone();
        self.ui.library.set_downloaded(path, false);
        self.archive.online = None;
        self.offer_undo(crate::app::ui_state::games::Undo::download(
            path.to_owned(),
            &name,
        ));
    }

    pub(super) fn keep_download(&mut self, path: &str) {
        self.ui.library.set_downloaded(path, true);
        self.archive.online = None;
    }

    pub(super) fn drop_download(&mut self, path: &str) {
        let downloaded = self
            .ui
            .library
            .find(path)
            .and_then(|game| game.archive.as_ref())
            .is_some_and(|copy| copy.downloaded);
        if !downloaded {
            self.archive.store.drop_copies([Path::new(path)]);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn settle_pending_download(&mut self) {
        if self.ui.games.pending_download().is_some()
            && let Some(undo) = self.ui.games.undo.take()
        {
            self.settle_undo(undo);
        }
    }

    fn unpack_archive(&mut self, download: ArchiveDownload) {
        let ArchiveDownload { game, packed } = download;
        let store = &self.archive.store;
        let path = listing(store, &game).path;
        #[cfg(not(target_arch = "wasm32"))]
        let keep = (store.clone(), hex(&game.sha256), game.file_name());
        let wanted = game.clone();
        let pending = self.threads.background("archive unpack", move || {
            let rom = wanted.unpack(&packed)?;
            #[cfg(not(target_arch = "wasm32"))]
            {
                let (store, key, name) = keep;
                if let Err(err) = store.keep(&key, &name, &rom) {
                    log::warn!("couldn't keep a Game Archive download: {err}");
                    return Err(UnpackError::Damaged);
                }
            }
            Ok(rom)
        });
        self.archive.unpacking.push((game, path, pending));
    }

    fn finish_unpacking(&mut self) {
        let mut at = 0;
        while at < self.archive.unpacking.len() {
            let Some(result) = self.archive.unpacking[at].2.try_take() else {
                at += 1;
                continue;
            };
            let (game, path, _) = self.archive.unpacking.remove(at);
            let play = self.archive.play_after.as_deref() == Some(path.as_str());
            match result {
                Ok(Ok(rom)) => self.archive_ready(&game, path, rom, play),
                Ok(Err(_)) | Err(_) => {
                    if play {
                        self.archive.play_after = None;
                    }
                    self.archive_failed(game.id, ArchiveFailure::Damaged);
                }
            }
        }
    }

    fn archive_ready(&mut self, game: &ArchiveGame, path: String, rom: Vec<u8>, play: bool) {
        #[cfg(target_arch = "wasm32")]
        if let Err(err) = self
            .archive
            .store
            .keep(&hex(&game.sha256), &game.file_name(), &rom)
        {
            log::warn!("couldn't keep a Game Archive download: {err}");
        }
        self.ui.library.set_downloaded(&path, true);
        self.archive.online = None;
        if !play {
            return;
        }
        self.archive.play_after = None;
        let _ = self.files.rom_tx.send(PickedFile {
            name: game.file_name(),
            data: rom.into(),
            #[cfg(not(target_arch = "wasm32"))]
            path: Some(path.into()),
            seen: None,
        });
    }

    fn archive_failed(&mut self, id: ArchiveId, failure: ArchiveFailure) {
        let path = self
            .ui
            .library
            .archive_games()
            .find(|game| {
                game.archive
                    .as_ref()
                    .is_some_and(|a| a.id == id.to_string())
            })
            .map(|game| (game.path.clone(), game.name.clone()));
        let Some((path, name)) = path else {
            return;
        };
        if self.archive.play_after.as_deref() == Some(path.as_str()) {
            self.archive.play_after = None;
        }
        self.toasts.error(failure_words(&name, failure));
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::app::ui_state::library::FileState;
    use crate::online::tests::{Wire, cut, push, sent, signed_in};
    use citrine_gb_net::{ARCHIVE_PART_BYTES, ArchiveKey, ClientMsg, RelayId, ServerMsg};
    use sha2::{Digest, Sha256};
    use std::cell::RefCell;
    use std::rc::Rc;

    const PREFIX: [u8; 16] = [4; 16];

    struct Setup {
        app: Citrine,
        wire: Rc<RefCell<Wire>>,
        ctx: egui::Context,
        dir: std::path::PathBuf,
    }

    fn setup(name: &str) -> Setup {
        let dir = crate::storage::scratch_dir().join(format!("citrine-archive-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        let mut app = crate::app::tests::test_app();
        let (online, wire, _) = signed_in(&RelayId::random());
        app.online = online;
        let account = citrine_gb_net::Account {
            online_id: citrine_gb_net::OnlineId::new(0x1234_5678).unwrap(),
            display_name: "Clara".into(),
            storage_used: 0,
            quota_bytes: 0,
            two_factor: false,
            codes_left: 0,
        };
        app.ui.settings.online.remember(
            &crate::online::address::ServerAddress::parse("relay.example").unwrap(),
            &RelayId::random(),
            "pw",
            &account,
            &Default::default(),
        );
        app.archive = ArchiveShelf::with_store(RomCopies::at(dir.clone()));
        Setup {
            app,
            wire,
            ctx: egui::Context::default(),
            dir,
        }
    }

    fn rom() -> Vec<u8> {
        (0..ARCHIVE_PART_BYTES as u32 + 5000)
            .map(|i| (i * 7 % 253) as u8)
            .collect()
    }

    fn game_of(rom: &[u8], packed: &[u8]) -> ArchiveGame {
        ArchiveGame {
            id: ArchiveId([9; 16]),
            name: "Pentris".into(),
            console: ArchiveConsole::GameBoyAdvance,
            size: rom.len() as u64,
            download: packed.len() as u64,
            sha256: Sha256::digest(rom).into(),
            sha1: [1; 20],
            crc32: 7,
        }
    }

    fn pack(rom: &[u8]) -> Vec<u8> {
        ruzstd::encoding::compress_to_vec(rom, ruzstd::encoding::CompressionLevel::Fastest)
    }

    impl Setup {
        fn step(&mut self) {
            self.app.online.update(web_time::Instant::now());
            self.app.follow_archive(&self.ctx);
        }

        fn list(&mut self, games: Vec<ArchiveGame>) {
            push(&self.wire, ServerMsg::Archive(games));
            self.step();
        }

        fn send_game(&mut self, game: &ArchiveGame, packed: &[u8]) {
            let key = ArchiveKey([2; 32]);
            let parts: Vec<&[u8]> = packed.chunks(ARCHIVE_PART_BYTES).collect();
            push(
                &self.wire,
                ServerMsg::ArchiveStart {
                    id: game.id,
                    key: key.clone(),
                    prefix: PREFIX,
                    parts: parts.len() as u32,
                },
            );
            for (index, part) in parts.iter().enumerate() {
                push(
                    &self.wire,
                    ServerMsg::ArchivePart {
                        id: game.id,
                        index: index as u32,
                        data: key.seal_part(&PREFIX, index as u32, part),
                    },
                );
            }
            for _ in 0..20 {
                self.step();
            }
        }

        fn path(&self) -> String {
            self.app
                .ui
                .library
                .archive_games()
                .next()
                .expect("an archive game")
                .path
                .clone()
        }
    }

    #[test]
    fn a_listed_game_joins_the_library_and_downloads_when_played() {
        let mut s = setup("play");
        let rom = rom();
        let packed = pack(&rom);
        let game = game_of(&rom, &packed);
        s.list(vec![game.clone()]);
        let path = s.path();
        let row = s.app.ui.library.find(&path).unwrap();
        assert_eq!(row.name, "Pentris");
        assert_eq!(row.console, Some(ConsoleKind::Gba));
        assert_eq!(row.file, FileState::Present);
        assert!(!row.archive.as_ref().unwrap().downloaded);
        assert!(path.ends_with(".gba"));
        s.app.play_library(path.clone());
        let gets: Vec<ClientMsg> = sent(&s.wire);
        assert!(gets.contains(&ClientMsg::ArchiveGet {
            id: game.id,
            from_part: 0,
        }));
        s.send_game(&game, &packed);
        let row = s.app.ui.library.find(&path).unwrap();
        assert!(row.archive.as_ref().unwrap().downloaded);
        assert_eq!(std::fs::read(&path).unwrap(), rom);
        let picked = s.app.files.rom_rx.try_recv().expect("the game starts");
        assert_eq!(&picked.data[..], &rom[..]);
        assert_eq!(picked.path.as_deref(), Some(Path::new(&path)));
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    #[test]
    fn a_download_that_doesnt_match_says_so_and_keeps_nothing() {
        let mut s = setup("damaged");
        let rom = rom();
        let packed = pack(&rom);
        let mut game = game_of(&rom, &packed);
        game.sha256 = [0; 32];
        s.list(vec![game.clone()]);
        let path = s.path();
        s.app.play_library(path.clone());
        s.send_game(&game, &packed);
        assert_eq!(
            s.app.toasts.texts(),
            ["Couldn't download Pentris: the download was damaged"]
        );
        assert!(!Path::new(&path).exists());
        assert!(s.app.files.rom_rx.try_recv().is_err());
    }

    #[test]
    fn offline_only_downloaded_games_stay_playable() {
        let mut s = setup("offline");
        let rom = rom();
        let packed = pack(&rom);
        let game = game_of(&rom, &packed);
        s.list(vec![game]);
        let path = s.path();
        s.app.online.leave();
        s.step();
        let row = s.app.ui.library.find(&path).unwrap();
        assert_eq!(row.file, FileState::Offline);
        assert_eq!(
            crate::app::tabs::games::library::tag_of(row),
            Some(crate::app::widgets::game_tile::Tag::ArchiveOffline)
        );
    }

    #[test]
    fn a_lost_line_marks_archive_games_offline_and_keeps_the_account() {
        let mut s = setup("lost-line");
        let rom = rom();
        let packed = pack(&rom);
        s.list(vec![game_of(&rom, &packed)]);
        let path = s.path();
        let _ = s.app.ui.recent.record(Path::new(&path), "Pentris", None);
        cut(&s.wire);
        s.step();
        assert!(!s.app.online.is_connected());
        assert!(s.app.online.account().is_some());
        let row = s.app.ui.library.find(&path).unwrap();
        assert_eq!(row.file, FileState::Offline);
        let recent = crate::app::mobile::games::recent_games(
            s.app.ui.recent.entries(),
            (None, None),
            crate::stats::today(),
            |_| false,
            |_| None,
        );
        let marks = crate::app::tabs::games::library::recent_marks(
            &s.app.ui.library,
            &s.app.online,
            &recent,
        );
        let (tag, archive) = marks[0];
        assert_eq!(
            tag,
            Some(crate::app::widgets::game_tile::Tag::ArchiveOffline)
        );
        assert_eq!(
            crate::app::widgets::game_tile::archive_badge(tag, archive),
            Some(crate::icons::CLOUD_SLASH)
        );
    }

    #[test]
    fn an_offline_archive_game_in_recent_opens_its_info_and_downloads_nothing() {
        let mut s = setup("recent-offline");
        let rom = rom();
        let packed = pack(&rom);
        s.list(vec![game_of(&rom, &packed)]);
        let path = s.path();
        let _ = s.app.ui.recent.record(Path::new(&path), "Pentris", None);
        cut(&s.wire);
        s.step();
        s.app
            .apply_games(Some(crate::app::events::GamesAction::Recent(
                path.clone().into(),
            )));
        assert_eq!(s.app.ui.games.info.as_deref(), Some(path.as_str()));
        assert!(s.app.toasts.texts().is_empty());
        assert!(s.app.archive.play_after.is_none());
        assert!(
            !sent(&s.wire)
                .iter()
                .any(|msg| matches!(msg, ClientMsg::ArchiveGet { .. }))
        );
    }

    fn downloaded(s: &Setup, path: &str) -> bool {
        s.app
            .ui
            .library
            .find(path)
            .unwrap()
            .archive
            .as_ref()
            .unwrap()
            .downloaded
    }

    #[test]
    fn delete_download_offers_undo_and_keeps_the_game_in_recent() {
        let mut s = setup("delete");
        let rom = rom();
        let packed = pack(&rom);
        let game = game_of(&rom, &packed);
        s.list(vec![game.clone()]);
        let path = s.path();
        s.app.play_library(path.clone());
        s.send_game(&game, &packed);
        let _ = s.app.ui.recent.record(Path::new(&path), "Pentris", None);
        s.app.delete_archive_download(&path);
        assert!(!downloaded(&s, &path));
        assert!(Path::new(&path).exists());
        let undo = s.app.ui.games.undo.as_ref().expect("an undo");
        assert_eq!(
            format!("{}{}", undo.name, undo.tail),
            "Pentris download deleted"
        );
        s.app.undo_library();
        assert!(downloaded(&s, &path));
        assert!(Path::new(&path).exists());
        s.app.delete_archive_download(&path);
        assert!(s.app.ui.games.expired_undo(0.0).is_none());
        let undo = s.app.ui.games.expired_undo(60.0).expect("the undo fades");
        s.app.settle_undo(undo);
        assert!(!Path::new(&path).exists());
        assert!(!downloaded(&s, &path));
        let library = &s.app.ui.library;
        assert_eq!(
            s.app
                .ui
                .recent
                .prune_except(|path| library.is_archive_row(path)),
            0
        );
        assert!(
            s.app
                .ui
                .recent
                .entries()
                .iter()
                .any(|entry| entry.path == Path::new(&path))
        );
        assert!(
            !s.app
                .ui
                .library
                .recent_gone(&s.app.files.rom_copies, Path::new(&path))
        );
        let _ = sent(&s.wire);
        s.app.load_recent(std::path::PathBuf::from(&path));
        assert!(sent(&s.wire).contains(&ClientMsg::ArchiveGet {
            id: game.id,
            from_part: 0,
        }));
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    #[test]
    fn play_before_the_undo_fades_keeps_the_download() {
        let mut s = setup("replay");
        let rom = rom();
        let packed = pack(&rom);
        let game = game_of(&rom, &packed);
        s.list(vec![game.clone()]);
        let path = s.path();
        s.app.play_library(path.clone());
        s.send_game(&game, &packed);
        while s.app.files.rom_rx.try_recv().is_ok() {}
        s.app.delete_archive_download(&path);
        s.app.archive.known = false;
        s.step();
        assert!(!downloaded(&s, &path));
        let _ = sent(&s.wire);
        s.app.play_library(path.clone());
        assert!(sent(&s.wire).is_empty());
        assert!(downloaded(&s, &path));
        assert!(s.app.ui.games.undo.is_none());
        assert!(Path::new(&path).exists());
        assert!(
            !s.app
                .toasts
                .texts()
                .iter()
                .any(|text| text.starts_with("Could not open"))
        );
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    #[test]
    fn clearing_the_archive_cache_takes_the_downloads_and_keeps_the_games() {
        let mut s = setup("clear-cache");
        let rom = rom();
        let packed = pack(&rom);
        let game = game_of(&rom, &packed);
        s.list(vec![game.clone()]);
        let path = s.path();
        s.app.play_library(path.clone());
        s.send_game(&game, &packed);
        let downloaded = |s: &Setup| {
            s.app
                .ui
                .library
                .find(&path)
                .and_then(|game| game.archive.as_ref())
                .map(|copy| copy.downloaded)
        };
        assert!(Path::new(&path).exists());
        assert_eq!(downloaded(&s), Some(true));
        s.app.clear_archive_downloads();
        s.step();
        assert!(!Path::new(&path).exists());
        assert_eq!(downloaded(&s), Some(false));
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    #[test]
    fn signing_out_or_a_revoke_takes_the_games_and_their_downloads() {
        let mut s = setup("revoke");
        let rom = rom();
        let packed = pack(&rom);
        let game = game_of(&rom, &packed);
        s.list(vec![game.clone()]);
        let path = s.path();
        s.app.play_library(path.clone());
        s.send_game(&game, &packed);
        assert!(Path::new(&path).exists());
        s.list(Vec::new());
        assert_eq!(s.app.ui.library.archive_games().count(), 0);
        assert!(!Path::new(&path).exists());
        s.list(vec![game.clone()]);
        s.app.play_library(path.clone());
        s.send_game(&game, &packed);
        assert!(Path::new(&path).exists());
        s.app.ui.settings.online.joined = None;
        s.step();
        assert_eq!(s.app.ui.library.archive_games().count(), 0);
        assert!(!Path::new(&path).exists());
    }

    #[test]
    fn a_local_copy_wins_over_the_archive_row() {
        let mut s = setup("local");
        let rom = rom();
        let packed = pack(&rom);
        let game = game_of(&rom, &packed);
        s.app.ui.library.record_opened(
            crate::app::ui_state::library::Opened {
                path: "/games/pentris.gba".into(),
                name: "Pentris".into(),
                console: Some(ConsoleKind::Gba),
                rom_key: Some(hex(&game.sha256)),
                sha1: Some(hex(&game.sha1)),
                crc32: Some(7),
            },
            1,
        );
        s.list(vec![game]);
        let shown: Vec<String> = s
            .app
            .ui
            .library
            .entries()
            .iter()
            .map(|game| game.path.clone())
            .collect();
        assert_eq!(shown, ["/games/pentris.gba"]);
    }
}

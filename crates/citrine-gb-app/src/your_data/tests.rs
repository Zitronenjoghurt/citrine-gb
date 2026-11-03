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

struct Home {
    root: PathBuf,
    saves: SaveStore,
    media: MediaStore,
    online: Online,
    settings: Settings,
    stats: Stats,
    recent: RecentRoms,
}

impl Home {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("citrine-your-data-{name}-{}", std::process::id()));
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
        }
    }

    fn fill(&mut self) {
        save(&self.saves, GAME, "battery", 100, b"battery");
        save(&self.saves, GAME, "slot0", 110, b"snapshot");
        save(&self.saves, OTHER, "auto3", 120, b"autosave");
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
    home.media
        .read_files(|media| add_media_files(&mut files, media));
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
            ("Saves", 3),
            ("Media", 2),
            ("Stats", 1),
            ("Chats", 2),
            ("Settings", 2),
            ("Recent games", 1),
            ("Game names", 1),
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

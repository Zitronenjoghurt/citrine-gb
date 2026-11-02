use citrine_gb::gb::{GameBoy, GbModel};
use citrine_gb::rom::Rom;
use citrine_gb_app::online::cloud::{AccountCopy, Choice, LocalSave, SaveStore, SyncState};
use citrine_gb_app::online::{Online, OnlineSettings, Phase, ServerAddress, Trust};
use citrine_gb_net::{RelayId, SavePayload};
use citrine_gb_server::{Config, Running, start};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use web_time::Instant;

const PASSWORD: &str = "lemon tree";
const ROM: &str = "5a17c0de5a17c0de5a17c0de5a17c0de5a17c0de5a17c0de5a17c0de5a17c0de";
const TITLE: &str = "Test Game";
const MARKER: &[u8] = b"PLAIN-SAVE-MARKER-1234567890";

struct Relay {
    runtime: tokio::runtime::Runtime,
    running: Option<Running>,
    address: ServerAddress,
    dir: tempfile::TempDir,
}

impl Relay {
    fn start(storage_mb: u64) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let vars: HashMap<&str, String> = HashMap::from([
            ("CITRINE_BIND", "127.0.0.1".into()),
            ("CITRINE_PORT", "0".into()),
            ("CITRINE_DISK_FLOOR_MB", "0".into()),
            ("CITRINE_DISK_FLOOR_PERCENT", "0".into()),
            (
                "CITRINE_DATA_DIR",
                dir.path().join("data").display().to_string(),
            ),
            ("CITRINE_WEB", "false".into()),
            ("CITRINE_RELAY", "true".into()),
            ("CITRINE_RELAY_PASSWORD", PASSWORD.into()),
            ("CITRINE_STORAGE_MB", storage_mb.to_string()),
        ]);
        let config = Config::from_vars(|k| vars.get(k).cloned()).unwrap();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let running = runtime.block_on(start(config)).unwrap();
        let address = ServerAddress::parse(&format!("127.0.0.1:{}", running.addr.port())).unwrap();
        Self {
            runtime,
            running: Some(running),
            address,
            dir,
        }
    }

    fn stored_files(&self) -> Vec<Vec<u8>> {
        let mut found = Vec::new();
        let mut dirs = vec![self.dir.path().join("data")];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.parent().and_then(Path::file_name) == Some("files".as_ref()) {
                    found.push(std::fs::read(&path).unwrap());
                }
            }
        }
        found
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        if let Some(running) = self.running.take() {
            self.runtime.block_on(running.stop());
        }
    }
}

struct Device {
    online: Online,
    store: SaveStore,
    _dir: tempfile::TempDir,
}

impl Device {
    fn new(settings: &OnlineSettings) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = SaveStore::in_dir(dir.path().to_path_buf());
        let mut online = Online::default();
        online.cloud_attach(store.clone());
        online.set_cloud(settings.cloud_owner(), true);
        Self {
            online,
            store,
            _dir: dir,
        }
    }

    fn sign_in(&mut self, settings: &OnlineSettings) {
        self.online
            .sign_in(settings.joined_relay().unwrap(), Instant::now());
        self.until("signing in", |d| d.online.is_connected());
    }

    fn go_offline(&mut self) {
        self.online.leave();
        self.online.update(Instant::now());
    }

    #[track_caller]
    fn until(&mut self, what: &str, done: impl Fn(&Device) -> bool) {
        let limit = Instant::now() + Duration::from_secs(20);
        loop {
            self.online.update(Instant::now());
            self.online.cloud_flush();
            if done(self) {
                return;
            }
            assert!(Instant::now() < limit, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn state(&self, save: &LocalSave) -> Option<SyncState> {
        self.online.cloud_state(ROM, save)
    }

    #[track_caller]
    fn synced(&mut self, saves: &[LocalSave]) {
        self.until("the saves to sync", |d| {
            saves.iter().all(|s| d.state(s) == Some(SyncState::Synced))
        });
    }

    fn battery(&self) -> Option<Vec<u8>> {
        self.store.load_battery(ROM)
    }
}

fn first_account(relay: &Relay) -> (Device, OnlineSettings) {
    let mut online = Online::default();
    online.probe(
        relay.address.clone(),
        PASSWORD.into(),
        Trust::default(),
        Instant::now(),
    );
    let limit = Instant::now() + Duration::from_secs(20);
    while online.phase() != Phase::Probed {
        online.update(Instant::now());
        assert!(Instant::now() < limit, "no server info");
        std::thread::sleep(Duration::from_millis(5));
    }
    online.create_account("Clara".into(), Instant::now());
    while !online.is_connected() {
        online.update(Instant::now());
        assert!(Instant::now() < limit, "no welcome");
        std::thread::sleep(Duration::from_millis(5));
    }
    let joined = online.take_joined().unwrap();
    let mut settings = OnlineSettings::default();
    settings.remember(
        &joined.address,
        &joined.relay_id,
        &joined.password,
        &joined.account,
        &joined.trust,
    );
    online.leave();
    let mut device = Device::new(&settings);
    device.sign_in(&settings);
    (device, settings)
}

fn relay_id(settings: &OnlineSettings) -> RelayId {
    settings.joined_entry().unwrap().relay_id().unwrap()
}

fn noise(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed;
    (0..len)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) as u8
        })
        .collect()
}

fn battery_bytes(tag: u8) -> Vec<u8> {
    let mut data = vec![tag; 8192];
    data[..MARKER.len()].copy_from_slice(MARKER);
    data
}

fn frame() -> Vec<u8> {
    vec![90; 160 * 144 * 4]
}

fn cloud_battery(settings: &OnlineSettings) -> Vec<u8> {
    let mut fresh = Device::new(settings);
    fresh.sign_in(settings);
    fresh.online.cloud_game(Some((ROM.into(), TITLE.into())));
    fresh.online.cloud_hold(ROM);
    fresh.until("the cloud battery save", |d| {
        d.state(&LocalSave::Battery) == Some(SyncState::Synced)
    });
    let data = fresh.battery().unwrap();
    fresh.go_offline();
    data
}

#[test]
fn a_save_made_on_one_device_arrives_on_the_other() {
    let relay = Relay::start(50);
    let (mut a, settings) = first_account(&relay);
    a.store.store_battery(ROM, &battery_bytes(1)).unwrap();
    a.store
        .store_snapshot(ROM, 0, &noise(30_000, 1), &frame(), GbModel::Cgb)
        .unwrap();
    a.store.set_snapshot_name(ROM, 0, "Boss door").unwrap();
    let auto = a
        .store
        .store_autosave(ROM, &noise(30_000, 2), &frame(), GbModel::Cgb, 5)
        .unwrap();
    a.online.cloud_game(Some((ROM.into(), TITLE.into())));
    a.synced(&[
        LocalSave::Battery,
        LocalSave::Snapshot(0),
        LocalSave::Autosave(auto),
    ]);
    a.go_offline();

    let mut b = Device::new(&settings);
    b.sign_in(&settings);
    b.online.cloud_game(Some((ROM.into(), TITLE.into())));
    b.online.cloud_hold(ROM);
    b.until("all three saves on the second device", |d| {
        d.battery().is_some()
            && d.store.snapshot_name(ROM, 0).is_some()
            && !d.store.autosave_ids(ROM).is_empty()
    });
    assert_eq!(b.battery().unwrap(), battery_bytes(1));
    assert_eq!(b.store.load_snapshot(ROM, 0).unwrap(), noise(30_000, 1));
    assert_eq!(b.store.snapshot_name(ROM, 0).as_deref(), Some("Boss door"));
    let id = b.store.autosave_ids(ROM)[0];
    assert_eq!(b.store.load_autosave(ROM, id).unwrap(), noise(30_000, 2));
    b.synced(&[
        LocalSave::Battery,
        LocalSave::Snapshot(0),
        LocalSave::Autosave(id),
    ]);

    let games = b.online.cloud_games();
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].title, TITLE);
    assert_eq!(games[0].saves, 3);

    let keys = relay_id(&settings).keys();
    let stored = relay.stored_files();
    assert_eq!(stored.len(), 3);
    for bytes in &stored {
        assert!(!bytes.windows(MARKER.len()).any(|w| w == MARKER));
        assert!(!bytes.windows(9).any(|w| w == b"Boss door"));
        assert!(keys.open_save(bytes).is_ok());
    }
    let stranger = RelayId::random().keys();
    assert!(stranger.open_save(&stored[0]).is_err());
}

#[test]
fn changes_on_two_offline_devices_ask_which_to_keep() {
    let relay = Relay::start(50);
    let (mut a, settings) = first_account(&relay);
    a.store.store_battery(ROM, &battery_bytes(1)).unwrap();
    a.synced(&[LocalSave::Battery]);
    a.go_offline();
    let mut b = Device::new(&settings);
    b.sign_in(&settings);
    b.online.cloud_game(Some((ROM.into(), TITLE.into())));
    b.online.cloud_hold(ROM);
    b.synced(&[LocalSave::Battery]);
    b.go_offline();

    let mut round = 1;
    for choice in [Choice::KeepHere, Choice::KeepCloud, Choice::KeepBoth] {
        round += 2;
        for device in [&mut a, &mut b] {
            device.sign_in(&settings);
            device.synced(&[LocalSave::Battery]);
            device.go_offline();
        }
        a.store.store_battery(ROM, &battery_bytes(round)).unwrap();
        b.store
            .store_battery(ROM, &battery_bytes(round + 1))
            .unwrap();
        a.sign_in(&settings);
        a.synced(&[LocalSave::Battery]);
        a.go_offline();
        b.sign_in(&settings);
        b.until("the conflict with the cloud's copy", |d| {
            d.online
                .cloud_conflict(ROM, &LocalSave::Battery)
                .is_some_and(|c| c.ready())
        });
        assert_eq!(b.state(&LocalSave::Battery), Some(SyncState::Conflict));
        let conflict = b.online.cloud_ask().expect("the open game asks");
        assert!(conflict.here.is_some());
        let slot = conflict.slot.clone();
        b.online.cloud_resolve(ROM, &slot, choice).unwrap();
        b.synced(&[LocalSave::Battery]);
        match choice {
            Choice::KeepHere => {
                assert_eq!(b.battery().unwrap(), battery_bytes(round + 1));
            }
            Choice::KeepCloud => {
                assert_eq!(b.battery().unwrap(), battery_bytes(round));
                assert!(b.online.cloud_take_reload());
            }
            Choice::KeepBoth => {
                assert_eq!(b.battery().unwrap(), battery_bytes(round + 1));
                let others = b.store.other_batteries(ROM);
                assert_eq!(others.len(), 1);
                let other = LocalSave::OtherBattery(others[0].token.clone());
                assert_eq!(
                    b.store
                        .read_payload(ROM, &other)
                        .unwrap()
                        .file("sav")
                        .unwrap(),
                    battery_bytes(round).as_slice()
                );
                assert!(others[0].name.as_deref().unwrap().contains(" from "));
                b.synced(&[other]);
            }
        }
        let kept = b.battery().unwrap();
        b.go_offline();
        assert_eq!(cloud_battery(&settings), kept);
    }
}

#[test]
fn a_full_cloud_keeps_the_local_save_and_says_so() {
    let relay = Relay::start(1);
    let (mut a, _settings) = first_account(&relay);
    for slot in 0..3 {
        a.store
            .store_snapshot(
                ROM,
                slot,
                &noise(400_000, slot as u64),
                &frame(),
                GbModel::Dmg,
            )
            .unwrap();
    }
    a.online.cloud_game(Some((ROM.into(), TITLE.into())));
    a.until("one save to be refused", |d| {
        (0..3).any(|s| d.state(&LocalSave::Snapshot(s)) == Some(SyncState::Full))
            && (0..3)
                .filter(|s| d.state(&LocalSave::Snapshot(*s)) == Some(SyncState::Synced))
                .count()
                == 2
    });
    let full = (0..3)
        .find(|s| a.state(&LocalSave::Snapshot(*s)) == Some(SyncState::Full))
        .unwrap();
    assert!(a.store.load_snapshot(ROM, full).is_some());

    let other = (0..3).find(|s| *s != full).unwrap();
    assert!(a.online.cloud_has_copy(ROM, &LocalSave::Snapshot(other)));
    a.online
        .cloud_forget(ROM, &LocalSave::Snapshot(other), true);
    a.store.delete_snapshot(ROM, other).unwrap();
    a.synced(&[LocalSave::Snapshot(full)]);
    assert_eq!(a.online.cloud_games()[0].saves, 2);
}

#[test]
fn deleting_here_only_keeps_the_cloud_copy() {
    let relay = Relay::start(50);
    let (mut a, settings) = first_account(&relay);
    a.store
        .store_snapshot(ROM, 1, &noise(20_000, 9), &frame(), GbModel::Dmg)
        .unwrap();
    a.online.cloud_game(Some((ROM.into(), TITLE.into())));
    a.synced(&[LocalSave::Snapshot(1)]);
    a.online.cloud_forget(ROM, &LocalSave::Snapshot(1), false);
    a.store.delete_snapshot(ROM, 1).unwrap();
    a.until("the local delete to settle", |d| {
        d.online.cloud_games().first().map(|g| g.saves) == Some(1)
            && d.store.load_snapshot(ROM, 1).is_none()
    });
    for _ in 0..50 {
        a.online.update(Instant::now());
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(a.store.load_snapshot(ROM, 1).is_none());
    a.go_offline();
    let mut b = Device::new(&settings);
    b.sign_in(&settings);
    b.online.cloud_game(Some((ROM.into(), TITLE.into())));
    b.until("the kept cloud copy", |d| {
        d.store.load_snapshot(ROM, 1).is_some()
    });
}

#[test]
fn saves_shrink_before_they_are_sealed() {
    let relay = Relay::start(50);
    let (mut a, settings) = first_account(&relay);
    let mut gb = GameBoy::new_empty(GbModel::Cgb);
    gb.load_rom(&Rom::new(include_bytes!("../homebrew/ucity.gbc")))
        .unwrap();
    for _ in 0..600 {
        gb.run_frame();
    }
    let sram = gb
        .poll_sram_dump(true, 0)
        .map(|d| d.as_slice().to_vec())
        .unwrap_or_else(|| vec![0; 32 * 1024]);
    let state = gb.dump_full(0).unwrap();
    let frame = gb.frame().as_slice().to_vec();
    a.store.store_battery(ROM, &sram).unwrap();
    a.store
        .store_snapshot(ROM, 0, &state, &frame, GbModel::Cgb)
        .unwrap();
    let auto = a
        .store
        .store_autosave(ROM, &state, &frame, GbModel::Cgb, 5)
        .unwrap();
    a.online.cloud_game(Some((ROM.into(), TITLE.into())));
    let saves = [
        ("battery save", LocalSave::Battery),
        ("snapshot slot", LocalSave::Snapshot(0)),
        ("autosave", LocalSave::Autosave(auto)),
    ];
    a.synced(&saves.clone().map(|(_, s)| s));
    let keys = relay_id(&settings).keys();
    for (what, save) in &saves {
        let payload: SavePayload = a.store.read_payload(ROM, save).unwrap();
        let sealed = keys.seal_save(&payload);
        println!(
            "{what}: {} bytes on the device, {} bytes compressed and sealed",
            payload.size(),
            sealed.len()
        );
        if matches!(save, LocalSave::Battery) {
            assert!(sealed.len() * 10 < payload.size());
        } else {
            assert!(sealed.len() < payload.size() + 128);
        }
    }
}

#[test]
fn a_new_device_with_its_own_save_asks_which_to_keep() {
    let relay = Relay::start(1);
    let (mut a, settings) = first_account(&relay);
    a.store.store_battery(ROM, &battery_bytes(1)).unwrap();
    a.store
        .store_snapshot(ROM, 0, &noise(250_000, 1), &frame(), GbModel::Cgb)
        .unwrap();
    a.store
        .store_snapshot(ROM, 1, &noise(250_000, 2), &frame(), GbModel::Cgb)
        .unwrap();
    a.online.cloud_game(Some((ROM.into(), TITLE.into())));
    a.synced(&[
        LocalSave::Battery,
        LocalSave::Snapshot(0),
        LocalSave::Snapshot(1),
    ]);
    a.go_offline();
    a.store.store_battery(ROM, &battery_bytes(2)).unwrap();
    a.store
        .store_snapshot(ROM, 2, &noise(700_000, 3), &frame(), GbModel::Cgb)
        .unwrap();

    let mut b = Device::new(&settings);
    let from = a._dir.path().join(ROM);
    let to = b._dir.path().join(ROM);
    std::fs::create_dir_all(&to).unwrap();
    for entry in std::fs::read_dir(&from).unwrap().flatten() {
        std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
    }
    b.online.cloud_game(Some((ROM.into(), TITLE.into())));
    b.online.cloud_hold(ROM);
    b.sign_in(&settings);
    b.until("the first-meeting conflict", |d| {
        d.online
            .cloud_conflict(ROM, &LocalSave::Battery)
            .is_some_and(|c| c.ready())
    });
    assert_eq!(b.state(&LocalSave::Battery), Some(SyncState::Conflict));
    assert!(b.online.cloud_ask().is_some());
    assert_eq!(b.battery().unwrap(), battery_bytes(2));
    b.until("the big snapshot to be refused", |d| {
        d.state(&LocalSave::Snapshot(2)) == Some(SyncState::Full)
    });
    assert_eq!(b.state(&LocalSave::Snapshot(0)), Some(SyncState::Synced));
}

#[test]
fn account_slots_reach_every_device_and_stay_out_of_the_game_list() {
    let relay = Relay::start(50);
    let (mut a, settings) = first_account(&relay);
    a.online.set_cloud(settings.cloud_owner(), false);
    a.online.account_put("stats", b"from a".to_vec());
    a.until("the copy to upload", |d| {
        d.online.account_state("stats") == Some(SyncState::Synced)
    });
    a.go_offline();
    let mut b = Device::new(&settings);
    b.sign_in(&settings);
    b.online.account_put("stats", b"from b".to_vec());
    b.online.account_put("other", b"not asked".to_vec());
    let mut seen: Vec<AccountCopy> = Vec::new();
    b.until("b's copies to upload and a's to arrive", |d| {
        d.online.account_state("stats") == Some(SyncState::Synced)
            && d.online.account_state("other") == Some(SyncState::Synced)
    });
    let limit = Instant::now() + Duration::from_secs(20);
    while seen.is_empty() {
        b.online.update(Instant::now());
        seen.extend(b.online.account_take("stats"));
        assert!(Instant::now() < limit, "a's copy never arrived");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].data, b"from a");
    b.go_offline();
    a.sign_in(&settings);
    let mut back: Vec<AccountCopy> = Vec::new();
    let limit = Instant::now() + Duration::from_secs(20);
    while back.is_empty() {
        a.online.update(Instant::now());
        a.online.cloud_flush();
        back.extend(a.online.account_take("stats"));
        assert!(Instant::now() < limit, "b's copy never arrived");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].data, b"from b");
    assert!(a.online.account_take("other").is_empty());
    assert!(a.online.cloud_games().is_empty());
    assert!(
        relay
            .stored_files()
            .iter()
            .all(|file| !file.windows(6).any(|w| w == b"from a"))
    );
}

#[test]
fn the_cloud_list_names_a_game_as_this_device_does() {
    let data = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/homebrew/libbet.gb")).unwrap();
    let header = Rom::new(&data).header().unwrap();
    let cartridge = header.title.trim().to_string();
    let name = citrine_gb_app::game_name::game_name(
        &header.sha1,
        None,
        &header.title,
        header.cgb_mode.is_cgb(),
    );
    assert_eq!(name, "Libbet and the Magic Floor");

    let relay = Relay::start(50);
    let (mut a, settings) = first_account(&relay);
    a.store.store_battery(ROM, &battery_bytes(1)).unwrap();
    a.online.cloud_game(Some((ROM.into(), cartridge.clone())));
    a.synced(&[LocalSave::Battery]);
    a.go_offline();

    let mut b = Device::new(&settings);
    b.sign_in(&settings);
    b.until("the cloud list", |d| !d.online.cloud_games().is_empty());
    assert_eq!(b.online.cloud_games()[0].title, cartridge);

    b.online.cloud_game(Some((ROM.into(), name.clone())));
    b.online.cloud_hold(ROM);
    b.until("the save on the second device", |d| d.battery().is_some());
    b.synced(&[LocalSave::Battery]);
    assert_eq!(b.online.cloud_games()[0].title, name);
}

#[test]
fn a_save_from_before_cloud_saves_names_its_game_in_the_cloud() {
    let name = "Libbet and the Magic Floor";
    let relay = Relay::start(50);
    let (mut a, settings) = first_account(&relay);
    a.store.store_battery(ROM, &battery_bytes(1)).unwrap();
    a.synced(&[LocalSave::Battery]);
    a.go_offline();

    let mut b = Device::new(&settings);
    b.sign_in(&settings);
    b.until("the cloud list", |d| !d.online.cloud_games().is_empty());
    assert_eq!(b.online.cloud_games()[0].title, "");
    b.go_offline();

    a.online.cloud_name(ROM, name);
    a.sign_in(&settings);
    a.until("the name to go up", |d| {
        d.state(&LocalSave::Battery) == Some(SyncState::Waiting)
    });
    a.synced(&[LocalSave::Battery]);

    let mut c = Device::new(&settings);
    c.sign_in(&settings);
    c.until("the cloud list", |d| !d.online.cloud_games().is_empty());
    assert_eq!(c.online.cloud_games()[0].title, name);
    assert_eq!(c.online.cloud_games()[0].saves, 1);
}

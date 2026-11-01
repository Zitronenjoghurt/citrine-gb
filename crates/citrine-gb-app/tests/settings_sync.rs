use citrine_gb_app::online::cloud::{SaveStore, SyncState};
use citrine_gb_app::online::synced::{SyncDoc, Values, now_millis};
use citrine_gb_app::online::{Online, OnlineSettings, Phase, ServerAddress};
use citrine_gb_server::{Config, Running, start};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use web_time::Instant;

const PASSWORD: &str = "lemon tree";

fn defaults() -> Values {
    Values::from([
        ("theme".into(), json!("Mocha")),
        ("matrix".into(), json!(false)),
        ("scale".into(), json!(2)),
    ])
}

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
    doc: SyncDoc,
    _dir: tempfile::TempDir,
}

impl Device {
    fn new(settings: &OnlineSettings) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = SaveStore::in_dir(dir.path().to_path_buf());
        let mut online = Online::default();
        online.cloud_attach(store);
        online.set_cloud(settings.cloud_owner(), true);
        Self {
            online,
            doc: SyncDoc::default(),
            _dir: dir,
        }
    }

    fn sign_in(&mut self, settings: &OnlineSettings) {
        self.online
            .sign_in(settings.joined_relay().unwrap(), Instant::now());
        self.until("signing in", |d| d.online.is_connected());
    }

    fn change(&mut self, key: &str, value: Value) {
        let mut current = self.doc.values();
        current.insert(key.into(), value);
        self.doc.note(&current, &defaults(), |_| true, now_millis());
        std::thread::sleep(Duration::from_millis(3));
    }

    fn value(&self, key: &str) -> Value {
        self.doc.values()[key].clone()
    }

    #[track_caller]
    fn settled(&mut self) {
        self.until("the settings to upload", |d| {
            d.online.account_state("settings") == Some(SyncState::Synced)
        });
    }

    #[track_caller]
    fn sees(&mut self, key: &str, value: Value) {
        self.until("the other device's settings", |d| d.value(key) == value);
        self.settled();
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
            self.online.sync_settings(&mut self.doc, true);
            if done(self) {
                return;
            }
            assert!(Instant::now() < limit, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn first_account(relay: &Relay) -> (Device, OnlineSettings) {
    let mut online = Online::default();
    online.probe(relay.address.clone(), PASSWORD.into(), Instant::now());
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
    );
    online.leave();
    let mut device = Device::new(&settings);
    device.sign_in(&settings);
    (device, settings)
}

#[test]
fn two_devices_end_equal_and_keep_the_last_change_per_setting() {
    let relay = Relay::start(50);
    let (mut desk, settings) = first_account(&relay);
    desk.doc
        .note(&defaults(), &defaults(), |_| true, now_millis());
    desk.change("theme", json!("Latte"));
    desk.settled();
    desk.go_offline();

    let mut phone = Device::new(&settings);
    phone
        .doc
        .note(&defaults(), &defaults(), |_| true, now_millis());
    phone.sign_in(&settings);
    phone.sees("theme", json!("Latte"));
    phone.go_offline();

    desk.change("theme", json!("Frappe"));
    phone.change("matrix", json!(true));
    phone.change("scale", json!(3));
    desk.change("scale", json!(4));

    phone.sign_in(&settings);
    phone.settled();
    phone.go_offline();
    desk.sign_in(&settings);
    desk.sees("matrix", json!(true));
    desk.go_offline();
    phone.sign_in(&settings);
    phone.sees("scale", json!(4));

    assert_eq!(desk.doc.values(), phone.doc.values());
    assert_eq!(desk.value("theme"), json!("Frappe"));
    assert_eq!(desk.value("matrix"), json!(true));
    assert_eq!(desk.value("scale"), json!(4));
}

#[test]
fn the_relay_never_sees_settings_in_plain_text() {
    let relay = Relay::start(50);
    let (mut desk, _settings) = first_account(&relay);
    desk.doc
        .note(&defaults(), &defaults(), |_| true, now_millis());
    desk.change("theme", json!("PLAIN-SETTINGS-MARKER"));
    desk.settled();
    let files = relay.stored_files();
    assert!(!files.is_empty());
    assert!(
        files
            .iter()
            .all(|file| !file.windows(21).any(|w| w == b"PLAIN-SETTINGS-MARKER"))
    );
    assert!(desk.online.cloud_games().is_empty());
    assert!(desk.online.account_bytes().is_some_and(|bytes| bytes > 0));
}

#[test]
fn turning_sync_off_stops_the_uploads() {
    let relay = Relay::start(50);
    let (mut desk, settings) = first_account(&relay);
    desk.doc
        .note(&defaults(), &defaults(), |_| true, now_millis());
    desk.settled();
    desk.online.sync_settings(&mut desk.doc, false);
    desk.change("theme", json!("Latte"));
    for _ in 0..200 {
        desk.online.update(Instant::now());
        desk.online.cloud_flush();
        desk.online.sync_settings(&mut desk.doc, false);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(desk.online.account_state("settings"), None);
    desk.go_offline();

    let mut phone = Device::new(&settings);
    phone
        .doc
        .note(&defaults(), &defaults(), |_| true, now_millis());
    phone.sign_in(&settings);
    phone.settled();
    assert_eq!(phone.value("theme"), json!("Mocha"));
}

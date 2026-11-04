use super::*;
use citrine_gb::console::ConsoleKind;

const KEY: &str = "CRASHGAME";

fn store(name: &str) -> (SaveStore, std::path::PathBuf) {
    let root =
        std::env::temp_dir().join(format!("citrine-crash-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    (SaveStore::at(root.clone()), root)
}

fn frame() -> Vec<u8> {
    vec![7; 160 * 144 * 4]
}

fn crash(saved_at: u64, state: u8, ram: u8) -> CrashSave {
    CrashSave::new(
        KEY.into(),
        saved_at,
        ConsoleKind::Gb,
        vec![state; 64],
        &frame(),
        Some(&BatterySave::raw(vec![ram; 32])),
    )
}

fn stored_before(store: &SaveStore, saved_at: u64) {
    store
        .store_crash_save(&crash(saved_at, 1, 1), 10)
        .expect("earlier save");
}

#[test]
fn a_crash_save_survives_encoding() {
    let with_battery = crash(1_700_000_000, 3, 4);
    assert_eq!(
        CrashSave::decode(&with_battery.encode()),
        Some(with_battery)
    );
    let without = CrashSave {
        battery: None,
        console: ConsoleKind::Gba,
        ..crash(5, 6, 7)
    };
    assert_eq!(CrashSave::decode(&without.encode()), Some(without));
}

#[test]
fn a_damaged_crash_save_is_ignored() {
    let raw = crash(9, 1, 2).encode();
    assert_eq!(CrashSave::decode(&raw[..raw.len() / 2]), None);
    assert_eq!(CrashSave::decode("not base64 at all"), None);
    assert_eq!(CrashSave::decode(""), None);
}

#[test]
fn storing_writes_the_battery_and_autosave_with_one_time() {
    let (store, root) = store("store");
    let id = store
        .store_crash_save(&crash(500, 2, 3), 10)
        .expect("store");
    assert_eq!(store.load_autosave(KEY, id), Some(vec![2; 64]));
    assert_eq!(store.autosave_infos(KEY)[0].saved_at, 500);
    assert_eq!(store.battery_saved_at(KEY), Some(500));
    assert_eq!(store.load_battery(KEY), Some(vec![3; 32]));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_newer_crash_save_is_taken() {
    let (store, root) = store("newer");
    stored_before(&store, 100);
    assert_eq!(store.recover_crash_save(&crash(200, 9, 8)).ok(), Some(true));
    let autosaves = store.autosave_infos(KEY);
    assert_eq!(autosaves.len(), 2, "the earlier autosave stays");
    assert_eq!(autosaves[0].saved_at, 200);
    assert_eq!(
        store.load_autosave(KEY, autosaves[0].slot),
        Some(vec![9; 64])
    );
    assert_eq!(store.battery_saved_at(KEY), Some(200));
    assert_eq!(store.load_battery(KEY), Some(vec![8; 32]));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn an_older_crash_save_is_dropped() {
    let (store, root) = store("older");
    stored_before(&store, 300);
    assert_eq!(
        store.recover_crash_save(&crash(200, 9, 8)).ok(),
        Some(false)
    );
    let autosaves = store.autosave_infos(KEY);
    assert_eq!(autosaves.len(), 1);
    assert_eq!(
        store.load_autosave(KEY, autosaves[0].slot),
        Some(vec![1; 64])
    );
    assert_eq!(store.battery_saved_at(KEY), Some(300));
    assert_eq!(store.load_battery(KEY), Some(vec![1; 32]));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_crash_save_already_in_the_store_is_not_added_twice() {
    let (store, root) = store("same");
    let saved = crash(400, 5, 6);
    store.store_crash_save(&saved, 10).expect("store");
    assert_eq!(store.recover_crash_save(&saved).ok(), Some(false));
    assert_eq!(store.autosave_infos(KEY).len(), 1);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_crash_save_for_a_new_game_is_taken() {
    let (store, root) = store("fresh");
    assert_eq!(store.recover_crash_save(&crash(10, 4, 4)).ok(), Some(true));
    assert_eq!(store.autosave_infos(KEY).len(), 1);
    assert_eq!(store.load_battery(KEY), Some(vec![4; 32]));
    let _ = std::fs::remove_dir_all(root);
}

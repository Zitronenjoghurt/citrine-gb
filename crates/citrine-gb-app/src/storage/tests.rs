use super::*;

const KEY: &str = "ABC123";

fn store(name: &str) -> (SaveStore, std::path::PathBuf) {
    let root =
        std::env::temp_dir().join(format!("citrine-store-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    (SaveStore::at(root.clone()), root)
}

fn settings_store(name: &str) -> SettingsStore {
    let root = std::env::temp_dir().join(format!(
        "citrine-settings-test-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    SettingsStore::at(root)
}

fn frame() -> Vec<u8> {
    (0..160 * 144 * 4).map(|i| (i % 256) as u8).collect()
}

#[test]
fn settings_round_trip() {
    let store = settings_store("round-trip");
    assert!(store.load::<Vec<String>>().is_none(), "nothing stored yet");

    let value = vec!["a".to_string(), "b".to_string()];
    store.store(&value).expect("store");
    assert_eq!(store.load::<Vec<String>>().as_ref(), Some(&value));
}

#[test]
fn settings_that_cannot_be_read_fall_back_to_none() {
    let store = settings_store("unreadable");
    store.store(&"a string").expect("store");

    assert!(store.load::<Vec<String>>().is_none());
}

#[test]
fn battery_round_trips() {
    let (store, root) = store("battery");
    assert!(store.load_battery(KEY).is_none());
    assert!(!store.has_battery(KEY));

    store.store_battery(KEY, &[1, 2, 3]).expect("store");
    assert!(store.has_battery(KEY));
    assert_eq!(store.load_battery(KEY).as_deref(), Some(&[1, 2, 3][..]));

    store.store_battery(KEY, &[4, 5]).expect("overwrite");
    assert_eq!(store.load_battery(KEY).as_deref(), Some(&[4, 5][..]));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn writes_leave_no_temp_file_behind() {
    let (store, root) = store("atomic");
    store.store_battery(KEY, &[7; 64]).expect("store");
    let leftovers: Vec<_> = std::fs::read_dir(root.join(KEY))
        .expect("dir")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "stray temp files: {leftovers:?}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn battery_records_when_it_was_written() {
    let (store, root) = store("battery-meta");
    assert!(store.battery_saved_at(KEY).is_none());
    store.store_battery(KEY, &[1]).expect("store");
    assert!(store.battery_saved_at(KEY).is_some_and(|t| t > 0));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn explicit_reimport_overwrites_a_stored_save() {
    let (store, root) = store("reimport");
    let rom_dir = root.join("roms");
    std::fs::create_dir_all(&rom_dir).expect("rom dir");
    let rom = rom_dir.join("game.gb");
    std::fs::write(&rom, b"rom").expect("rom");
    std::fs::write(rom_dir.join("game.sav"), b"from-file").expect("sav");

    assert!(SaveStore::has_legacy_save(&rom));
    store.store_battery(KEY, b"stored").expect("stored");

    assert!(!store.import_legacy_save_if_new(KEY, &rom));
    assert_eq!(store.load_battery(KEY).as_deref(), Some(&b"stored"[..]));

    assert!(store.import_legacy_save(KEY, &rom));
    assert_eq!(store.load_battery(KEY).as_deref(), Some(&b"from-file"[..]));
    assert!(rom_dir.join("game.sav").exists(), "original is kept");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn no_legacy_file_means_no_import() {
    let (store, root) = store("no-legacy");
    let rom_dir = root.join("roms");
    std::fs::create_dir_all(&rom_dir).expect("rom dir");
    let rom = rom_dir.join("game.gb");
    std::fs::write(&rom, b"rom").expect("rom");

    assert!(!SaveStore::has_legacy_save(&rom));
    assert!(!store.import_legacy_save_if_new(KEY, &rom));
    assert!(store.load_battery(KEY).is_none());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn legacy_save_is_adopted_once_and_original_kept() {
    let (store, root) = store("legacy");
    let rom_dir = root.join("roms");
    std::fs::create_dir_all(&rom_dir).expect("rom dir");
    let rom = rom_dir.join("game.gb");
    std::fs::write(&rom, b"rom").expect("rom");
    std::fs::write(rom_dir.join("game.sav"), b"old-save").expect("sav");

    assert!(
        store.import_legacy_save_if_new(KEY, &rom),
        "first import runs"
    );
    assert_eq!(store.load_battery(KEY).as_deref(), Some(&b"old-save"[..]));
    assert!(rom_dir.join("game.sav").exists(), "original is kept");

    store.store_battery(KEY, b"newer").expect("newer");
    assert!(
        !store.import_legacy_save_if_new(KEY, &rom),
        "does not re-import"
    );
    assert_eq!(store.load_battery(KEY).as_deref(), Some(&b"newer"[..]));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn snapshot_round_trips_with_metadata() {
    let (store, root) = store("snapshot");
    assert!(store.snapshot_info(KEY, 0).is_none());

    store
        .store_snapshot(KEY, 3, b"state-bytes", &frame(), GbModel::Dmg)
        .expect("store");
    assert_eq!(
        store.load_snapshot(KEY, 3).as_deref(),
        Some(&b"state-bytes"[..])
    );

    let info = store.snapshot_info(KEY, 3).expect("filled");
    assert_eq!(info.slot, 3);
    assert!(info.saved_at > 0, "timestamp recorded");
    assert_eq!(info.thumbnail.len(), THUMB_WIDTH * THUMB_HEIGHT * 4);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn slots_are_unbounded_and_listed_in_order() {
    let (store, root) = store("unbounded");
    for slot in [0, 5, 99, 1000] {
        store
            .store_snapshot(KEY, slot, b"state", &frame(), GbModel::Dmg)
            .expect("store");
    }
    assert_eq!(store.used_slots(KEY), vec![0, 5, 99, 1000]);
    assert_eq!(store.snapshot_infos(KEY).len(), 4);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn next_free_slot_fills_gaps_before_extending() {
    let (store, root) = store("nextfree");
    assert_eq!(store.next_free_slot(KEY), 0);

    for slot in [0, 1, 2] {
        store
            .store_snapshot(KEY, slot, b"state", &frame(), GbModel::Dmg)
            .expect("store");
    }
    assert_eq!(store.next_free_slot(KEY), 3);

    store.delete_snapshot(KEY, 1).expect("delete");
    assert_eq!(store.next_free_slot(KEY), 1, "reuses the gap");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn thumbnails_are_stored_compressed() {
    let (store, root) = store("compress");
    store
        .store_snapshot(KEY, 0, b"state", &frame(), GbModel::Dmg)
        .expect("store");

    let raw = THUMB_WIDTH * THUMB_HEIGHT * 4;
    let on_disk = std::fs::metadata(root.join(KEY).join("slot0.thumb"))
        .expect("thumb")
        .len() as usize;
    assert!(
        on_disk < raw / 2,
        "expected compression, got {on_disk} of {raw} bytes"
    );

    let info = store.snapshot_info(KEY, 0).expect("filled");
    assert_eq!(info.thumbnail.len(), raw);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn snapshots_and_battery_share_one_namespace() {
    let (store, root) = store("namespace");
    store.store_battery(KEY, b"sram").expect("battery");
    store
        .store_snapshot(KEY, 0, b"state", &frame(), GbModel::Dmg)
        .expect("snapshot");
    assert!(root.join(KEY).join("battery.sav").exists());
    assert!(root.join(KEY).join("slot0.snap").exists());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_slot_with_no_name_counts_from_one() {
    let (store, root) = store("unnamed");
    store
        .store_snapshot(KEY, 2, b"state", &frame(), GbModel::Dmg)
        .expect("store");

    let info = store.snapshot_info(KEY, 2).expect("filled");
    assert_eq!(info.name, None, "nothing was written for a fresh slot");
    assert_eq!(
        info.display_name(),
        "Slot 3",
        "the third slot reads as Slot 3"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_save_from_before_one_based_names_still_loads() {
    let (store, root) = store("old-layout");
    let dir = root.join(KEY);
    std::fs::create_dir_all(&dir).expect("dir");
    store
        .store_snapshot(KEY, 0, b"old state", &frame(), GbModel::Dmg)
        .expect("store");
    std::fs::write(dir.join("slot1.name"), "Slot 0").expect("old name");
    store
        .store_snapshot(KEY, 1, b"named", &frame(), GbModel::Dmg)
        .expect("store");

    assert_eq!(store.used_slots(KEY), vec![0, 1]);
    assert_eq!(
        store.load_snapshot(KEY, 0).as_deref(),
        Some(&b"old state"[..])
    );
    let infos = store.snapshot_infos(KEY);
    assert_eq!(infos[0].display_name(), "Slot 1");
    assert_eq!(
        infos[1].display_name(),
        "Slot 0",
        "a stored name is kept as written"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn naming_a_slot_sticks() {
    let (store, root) = store("named");
    store
        .store_snapshot(KEY, 0, b"state", &frame(), GbModel::Dmg)
        .expect("store");

    store
        .set_snapshot_name(KEY, 0, "  Before the boss  ")
        .expect("name");

    let info = store.snapshot_info(KEY, 0).expect("filled");
    assert_eq!(info.name.as_deref(), Some("Before the boss"), "trimmed");
    assert_eq!(info.display_name(), "Before the boss");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn overwriting_a_slot_keeps_its_name() {
    let (store, root) = store("named-overwrite");
    store
        .store_snapshot(KEY, 1, b"first", &frame(), GbModel::Dmg)
        .expect("store");
    store.set_snapshot_name(KEY, 1, "Checkpoint").expect("name");

    store
        .store_snapshot(KEY, 1, b"second", &frame(), GbModel::Dmg)
        .expect("overwrite");

    let info = store.snapshot_info(KEY, 1).expect("filled");
    assert_eq!(info.display_name(), "Checkpoint");
    assert_eq!(store.load_snapshot(KEY, 1).as_deref(), Some(&b"second"[..]));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_blank_name_goes_back_to_the_default() {
    let (store, root) = store("name-cleared");
    store
        .store_snapshot(KEY, 0, b"state", &frame(), GbModel::Dmg)
        .expect("store");
    store.set_snapshot_name(KEY, 0, "Temporary").expect("name");

    store.set_snapshot_name(KEY, 0, "   ").expect("clear");

    let info = store.snapshot_info(KEY, 0).expect("filled");
    assert_eq!(info.name, None);
    assert_eq!(info.display_name(), "Slot 1");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_name_matching_the_default_is_not_stored() {
    let (store, root) = store("name-default");
    store
        .store_snapshot(KEY, 4, b"state", &frame(), GbModel::Dmg)
        .expect("store");

    store.set_snapshot_name(KEY, 4, "Slot 5").expect("name");

    assert_eq!(store.snapshot_name(KEY, 4), None);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn names_are_capped() {
    let (store, root) = store("name-length");
    store
        .store_snapshot(KEY, 0, b"state", &frame(), GbModel::Dmg)
        .expect("store");

    let long = "x".repeat(MAX_SAVE_NAME * 3);
    store.set_snapshot_name(KEY, 0, &long).expect("name");

    let stored = store.snapshot_name(KEY, 0).expect("named");
    assert_eq!(stored.chars().count(), MAX_SAVE_NAME);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn deleting_a_slot_takes_its_name_with_it() {
    let (store, root) = store("name-delete");
    store
        .store_snapshot(KEY, 0, b"state", &frame(), GbModel::Dmg)
        .expect("store");
    store.set_snapshot_name(KEY, 0, "Gone soon").expect("name");

    store.delete_snapshot(KEY, 0).expect("delete");

    assert_eq!(store.snapshot_name(KEY, 0), None, "no orphaned name file");
    assert!(store.snapshot_info(KEY, 0).is_none());

    store
        .store_snapshot(KEY, 0, b"fresh", &frame(), GbModel::Dmg)
        .expect("store");
    assert_eq!(
        store.snapshot_info(KEY, 0).expect("filled").display_name(),
        "Slot 1"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn names_do_not_look_like_snapshot_slots() {
    let (store, root) = store("name-listing");
    store
        .store_snapshot(KEY, 7, b"state", &frame(), GbModel::Dmg)
        .expect("store");
    store.set_snapshot_name(KEY, 7, "Named").expect("name");

    assert_eq!(
        store.used_slots(KEY),
        vec![7],
        "the name file must not register as another slot"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_battery_save_has_no_name_until_one_is_given() {
    let (store, root) = store("battery-unnamed");
    store.store_battery(KEY, b"sram").expect("battery");
    assert_eq!(store.battery_name(KEY), None);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn renaming_the_battery_save_sticks_across_writes() {
    let (store, root) = store("battery-named");
    store.store_battery(KEY, b"first").expect("battery");

    store
        .set_battery_name(KEY, "  Main run  ", "POKEMON_SLV")
        .expect("name");
    store.store_battery(KEY, b"second").expect("overwrite");

    assert_eq!(store.battery_name(KEY).as_deref(), Some("Main run"));
    assert_eq!(store.load_battery(KEY).as_deref(), Some(&b"second"[..]));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_blank_or_default_battery_name_is_not_stored() {
    let (store, root) = store("battery-name-default");
    store
        .set_battery_name(KEY, "Nuzlocke", "TETRIS")
        .expect("name");

    store.set_battery_name(KEY, "   ", "TETRIS").expect("clear");
    assert_eq!(store.battery_name(KEY), None);

    store
        .set_battery_name(KEY, "TETRIS", "TETRIS")
        .expect("default");
    assert_eq!(store.battery_name(KEY), None);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn battery_names_are_capped_and_do_not_look_like_slots() {
    let (store, root) = store("battery-name-cap");
    let long = "y".repeat(MAX_SAVE_NAME * 2);
    store.set_battery_name(KEY, &long, "TETRIS").expect("name");

    assert_eq!(
        store.battery_name(KEY).map(|n| n.chars().count()),
        Some(MAX_SAVE_NAME)
    );
    assert!(store.used_slots(KEY).is_empty());
    assert!(!store.has_battery(KEY), "a name alone is not a save");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn thumbnail_averages_each_2x2_block() {
    let mut frame = vec![0u8; 160 * 144 * 4];
    for (i, dx) in [(0usize, 0usize), (1, 4), (160, 8), (161, 12)] {
        frame[i * 4] = dx as u8;
    }
    let thumb = downscale(&frame);
    assert_eq!(thumb.len(), THUMB_WIDTH * THUMB_HEIGHT * 4);
    assert_eq!(thumb[0], 6);
}

#[test]
fn autosaves_keep_only_the_newest() {
    let (store, root) = store("auto-keep");
    for n in 0..5u8 {
        store
            .store_autosave(KEY, &[n], &frame(), GbModel::Dmg, 3)
            .expect("autosave");
    }

    assert_eq!(store.autosave_ids(KEY), vec![2, 3, 4]);
    let newest_first: Vec<usize> = store.autosave_infos(KEY).iter().map(|i| i.slot).collect();
    assert_eq!(newest_first, vec![4, 3, 2]);
    assert_eq!(store.load_autosave(KEY, 4).as_deref(), Some(&[4u8][..]));
    assert_eq!(store.load_autosave(KEY, 1), None);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_lower_limit_drops_the_oldest_at_once() {
    let (store, root) = store("auto-shrink");
    for _ in 0..4 {
        store
            .store_autosave(KEY, b"state", &frame(), GbModel::Dmg, 10)
            .expect("autosave");
    }
    store.prune_autosaves(KEY, 1).expect("prune");
    assert_eq!(store.autosave_ids(KEY), vec![3]);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn autosaves_never_touch_manual_snapshots() {
    let (store, root) = store("auto-apart");
    store
        .store_snapshot(KEY, 0, b"mine", &frame(), GbModel::Dmg)
        .expect("snapshot");
    store.set_snapshot_name(KEY, 0, "Boss fight").expect("name");

    for _ in 0..3 {
        store
            .store_autosave(KEY, b"auto", &frame(), GbModel::Dmg, 1)
            .expect("autosave");
    }

    assert_eq!(store.used_slots(KEY), vec![0]);
    assert_eq!(store.next_free_slot(KEY), 1);
    assert_eq!(store.load_snapshot(KEY, 0).as_deref(), Some(&b"mine"[..]));
    assert_eq!(store.snapshot_name(KEY, 0).as_deref(), Some("Boss fight"));
    assert_eq!(store.autosave_ids(KEY).len(), 1);

    store.delete_snapshot(KEY, 0).expect("delete");
    assert_eq!(store.autosave_ids(KEY).len(), 1);
    let _ = std::fs::remove_dir_all(root);
}

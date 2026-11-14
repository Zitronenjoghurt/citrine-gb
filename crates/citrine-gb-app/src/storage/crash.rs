use super::autosaves::autosave_thumbnail;
use super::battery::battery_meta;
use super::{BATTERY, BATTERY_META, SaveStore};
#[cfg(any(target_arch = "wasm32", test))]
use super::{decode_meta, encode_meta};
#[cfg(any(target_arch = "wasm32", test))]
use base64::Engine;
use citrine_gb::console::ConsoleKind;
use citrine_gb::persistence::battery_save::BatterySave;

#[cfg(any(target_arch = "wasm32", test))]
const VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashSave {
    pub rom_key: String,
    pub saved_at: u64,
    pub console: ConsoleKind,
    pub state: Vec<u8>,
    pub thumb: Vec<u8>,
    pub battery: Option<(Vec<u8>, Vec<u8>)>,
}

impl CrashSave {
    pub fn new(
        rom_key: String,
        saved_at: u64,
        console: ConsoleKind,
        state: Vec<u8>,
        frame_rgba: &[u8],
        battery: Option<&BatterySave>,
    ) -> Self {
        Self {
            rom_key,
            saved_at,
            console,
            state,
            thumb: autosave_thumbnail(frame_rgba, console),
            battery: battery.map(|save| (save.to_file(), battery_meta(save, saved_at))),
        }
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub fn encode(&self) -> String {
        let mut out = vec![VERSION];
        out.extend(self.saved_at.to_le_bytes());
        out.push(encode_meta(0, self.console)[8]);
        for part in [self.rom_key.as_bytes(), &self.state, &self.thumb] {
            push_part(&mut out, part);
        }
        match &self.battery {
            Some((file, meta)) => {
                out.push(1);
                push_part(&mut out, file);
                push_part(&mut out, meta);
            }
            None => out.push(0),
        }
        base64::engine::general_purpose::STANDARD.encode(out)
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub fn decode(raw: &str) -> Option<Self> {
        let data = base64::engine::general_purpose::STANDARD.decode(raw).ok()?;
        let mut reader = Reader(&data);
        if reader.take(1)? != [VERSION] {
            return None;
        }
        let saved_at = u64::from_le_bytes(reader.take(8)?.try_into().ok()?);
        let mut meta = [0; 9];
        meta[8] = reader.take(1)?[0];
        let console = decode_meta(&meta).1?;
        let rom_key = String::from_utf8(reader.part()?.to_vec()).ok()?;
        let state = reader.part()?.to_vec();
        let thumb = reader.part()?.to_vec();
        let battery = match reader.take(1)? {
            [1] => Some((reader.part()?.to_vec(), reader.part()?.to_vec())),
            [0] => None,
            _ => return None,
        };
        reader.0.is_empty().then_some(Self {
            rom_key,
            saved_at,
            console,
            state,
            thumb,
            battery,
        })
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn push_part(out: &mut Vec<u8>, part: &[u8]) {
    out.extend(u32::try_from(part.len()).unwrap_or(u32::MAX).to_le_bytes());
    out.extend(part);
}

#[cfg(any(target_arch = "wasm32", test))]
struct Reader<'a>(&'a [u8]);

#[cfg(any(target_arch = "wasm32", test))]
impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        if self.0.len() < count {
            return None;
        }
        let (head, rest) = self.0.split_at(count);
        self.0 = rest;
        Some(head)
    }

    fn part(&mut self) -> Option<&'a [u8]> {
        let len = u32::from_le_bytes(self.take(4)?.try_into().ok()?);
        self.take(usize::try_from(len).ok()?)
    }
}

impl SaveStore {
    pub fn store_crash_save(&self, crash: &CrashSave, keep: usize) -> std::io::Result<usize> {
        let battery = match &crash.battery {
            Some((file, meta)) => self.write_battery_files(&crash.rom_key, file, meta),
            None => Ok(()),
        };
        let id = self.store_autosave_at(
            &crash.rom_key,
            &crash.state,
            &crash.thumb,
            crash.console,
            crash.saved_at,
            keep,
        )?;
        battery.map(|()| id)
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub fn recover_crash_save(&self, crash: &CrashSave) -> std::io::Result<bool> {
        let rom_key = &crash.rom_key;
        let mut taken = false;
        if let Some((file, meta)) = &crash.battery
            && self
                .battery_saved_at(rom_key)
                .is_none_or(|at| at < crash.saved_at)
        {
            self.write_battery_files(rom_key, file, meta)?;
            taken = true;
        }
        if self
            .newest_autosave_at(rom_key)
            .is_none_or(|at| at < crash.saved_at)
        {
            let id = self.next_autosave_id(rom_key);
            self.write_autosave(
                rom_key,
                id,
                &crash.state,
                &crash.thumb,
                crash.console,
                crash.saved_at,
            )?;
            taken = true;
        }
        Ok(taken)
    }

    fn write_battery_files(&self, rom_key: &str, file: &[u8], meta: &[u8]) -> std::io::Result<()> {
        self.write(rom_key, BATTERY, file)?;
        self.write(rom_key, BATTERY_META, meta)
    }
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn a_crash_save_from_version_0_11_still_decodes() {
        let crash =
            CrashSave::decode("AQDxU2UAAAAAAgQAAABHQU1FAgAAAHN0AgAAAHRoAQIAAABzdgIAAABtdA==")
                .expect("decodes");
        assert_eq!(crash.saved_at, 1_700_000_000);
        assert_eq!(crash.console, ConsoleKind::Gba);
        assert_eq!(crash.rom_key, "GAME");
        assert_eq!(crash.state, b"st");
        assert_eq!(crash.thumb, b"th");
        assert_eq!(crash.battery, Some((b"sv".to_vec(), b"mt".to_vec())));
    }

    #[test]
    fn an_eight_megabyte_crash_save_round_trips() {
        let big: Vec<u8> = (0..8u32 << 20).map(|i| (i % 251) as u8).collect();
        let saved = CrashSave::new(
            KEY.into(),
            300,
            ConsoleKind::Gba,
            big.clone(),
            &vec![7; 240 * 160 * 4],
            Some(&BatterySave::raw(big.clone())),
        );
        let back = CrashSave::decode(&saved.encode()).expect("decodes");
        assert_eq!(back.state, big);
        assert_eq!(
            back.battery.as_ref().map(|(file, _)| file.len()),
            Some(big.len())
        );

        let (store, root) = store("big");
        let id = store.store_crash_save(&back, 10).expect("store");
        assert_eq!(store.load_autosave(KEY, id), Some(big));
        let _ = std::fs::remove_dir_all(root);
    }
}

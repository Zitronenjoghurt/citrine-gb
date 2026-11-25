use base64::Engine;
use std::collections::{BTreeMap, BTreeSet};

pub const LOCAL_PREFIX: &str = "citrine/";
const DEV_NAMESPACE: &str = "dev";
pub const LOCAL_MAX_CHARS: usize = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Put(String, Vec<u8>),
    Delete(String),
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Batch {
    pub ops: Vec<Op>,
    pub moved: Vec<String>,
}

impl Batch {
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty() && self.moved.is_empty()
    }
}

#[derive(Default)]
pub struct Mirror {
    files: BTreeMap<String, Vec<u8>>,
    touched: BTreeSet<String>,
    pending: Batch,
}

pub fn key(namespace: &str, name: &str) -> String {
    format!("{namespace}/{name}")
}

pub fn local_key(namespace: &str, name: &str) -> String {
    format!("{LOCAL_PREFIX}{namespace}/{name}")
}

fn stays_local(namespace: &str) -> bool {
    use super::kinds::{Folder, Root};
    use strum::IntoEnumIterator;
    namespace == DEV_NAMESPACE
        || Folder::iter().any(|folder| folder.root() == Root::Data && folder.name() == namespace)
}

pub fn fits_local(encoded: &str) -> bool {
    encoded.len() <= LOCAL_MAX_CHARS
}

impl Mirror {
    pub fn adopt_local(&mut self, entries: impl IntoIterator<Item = (String, String)>) {
        for (local, raw) in entries {
            let Some(path) = local.strip_prefix(LOCAL_PREFIX) else {
                continue;
            };
            let Some((namespace, _)) = path.split_once('/') else {
                continue;
            };
            if stays_local(namespace) {
                continue;
            }
            let Ok(data) = base64::engine::general_purpose::STANDARD.decode(raw) else {
                continue;
            };
            self.files.insert(path.to_string(), data.clone());
            self.touched.insert(path.to_string());
            self.pending.ops.push(Op::Put(path.to_string(), data));
            self.pending.moved.push(local);
        }
    }

    pub fn fill(&mut self, stored: impl IntoIterator<Item = (String, Vec<u8>)>) {
        for (key, data) in stored {
            if !self.touched.contains(&key) {
                self.files.insert(key, data);
            }
        }
    }

    pub fn fill_and_recover(
        &mut self,
        stored: impl IntoIterator<Item = (String, Vec<u8>)>,
        mut back_to_local: impl FnMut(&str, &str) -> bool,
    ) {
        let mut saves = Vec::new();
        for (key, data) in stored {
            let namespace = key.split_once('/').map_or("", |(namespace, _)| namespace);
            if !stays_local(namespace) {
                saves.push((key, data));
                continue;
            }
            let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
            if back_to_local(&format!("{LOCAL_PREFIX}{key}"), &encoded) {
                self.pending.ops.push(Op::Delete(key));
            }
        }
        self.fill(saves);
    }

    pub fn read(&self, key: &str) -> Option<&[u8]> {
        self.files.get(key).map(Vec::as_slice)
    }

    pub fn write(&mut self, key: String, data: &[u8]) {
        self.files.insert(key.clone(), data.to_vec());
        self.touched.insert(key.clone());
        self.pending.ops.push(Op::Put(key, data.to_vec()));
    }

    pub fn delete(&mut self, key: String) {
        self.files.remove(&key);
        self.touched.insert(key.clone());
        self.pending.ops.push(Op::Delete(key));
    }

    pub fn list(&self, namespace: &str) -> Vec<String> {
        let prefix = format!("{namespace}/");
        self.files
            .range(prefix.clone()..)
            .map_while(|(key, _)| key.strip_prefix(&prefix))
            .map(str::to_owned)
            .collect()
    }

    pub fn namespaces(&self) -> Vec<String> {
        let mut found: Vec<String> = self
            .files
            .keys()
            .filter_map(|key| {
                key.split_once('/')
                    .map(|(namespace, _)| namespace.to_owned())
            })
            .collect();
        found.sort();
        found.dedup();
        found
    }

    pub fn remove_after_commit(&mut self, local: String) {
        self.pending.moved.push(local);
    }

    pub fn take_pending(&mut self) -> Batch {
        std::mem::take(&mut self.pending)
    }

    pub fn retry(&mut self, batch: Batch) {
        let mut keys: Vec<String> = batch
            .ops
            .into_iter()
            .map(|op| match op {
                Op::Put(key, _) | Op::Delete(key) => key,
            })
            .collect();
        keys.sort();
        keys.dedup();
        for key in keys {
            let op = match self.files.get(&key) {
                Some(data) => Op::Put(key, data.clone()),
                None => Op::Delete(key),
            };
            self.pending.ops.push(op);
        }
        self.pending.moved.extend(batch.moved);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GAME: &str = "ABCDEF0123";

    fn encoded(data: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(data)
    }

    fn filled_local_storage() -> Vec<(String, String)> {
        vec![
            (local_key(GAME, "battery.sav"), encoded(&[1, 2, 3, 4])),
            (
                local_key(GAME, "battery.meta"),
                encoded(&7u64.to_le_bytes()),
            ),
            (local_key(GAME, "slot0.snap"), encoded(&[9; 3000])),
            (local_key(GAME, "slot0.name"), encoded(b"Before the boss")),
            (local_key("borrowed-FF", "battery.sav"), encoded(&[5; 32])),
            (local_key("settings", "settings.json"), encoded(b"{}")),
            ("other/app".to_string(), "kept".to_string()),
            (local_key(GAME, "broken.sav"), "not base64!".to_string()),
        ]
    }

    #[test]
    fn every_save_in_local_storage_moves_with_its_bytes() {
        let local = filled_local_storage();
        let mut mirror = Mirror::default();
        mirror.adopt_local(local.clone());
        let batch = mirror.take_pending();

        let moved: Vec<&str> = batch.moved.iter().map(String::as_str).collect();
        assert_eq!(
            moved,
            [
                "citrine/ABCDEF0123/battery.sav",
                "citrine/ABCDEF0123/battery.meta",
                "citrine/ABCDEF0123/slot0.snap",
                "citrine/ABCDEF0123/slot0.name",
                "citrine/borrowed-FF/battery.sav",
            ]
        );
        assert_eq!(batch.ops.len(), moved.len());
        for (local_name, raw) in &local {
            let Some(path) = local_name.strip_prefix(LOCAL_PREFIX) else {
                continue;
            };
            let Ok(data) = base64::engine::general_purpose::STANDARD.decode(raw) else {
                assert_eq!(mirror.read(path), None);
                continue;
            };
            if path.starts_with("settings/") {
                assert_eq!(mirror.read(path), None);
                continue;
            }
            assert_eq!(mirror.read(path), Some(&data[..]), "{path}");
            assert!(
                batch.ops.contains(&Op::Put(path.to_string(), data)),
                "{path}"
            );
        }
        assert_eq!(
            mirror.list(GAME),
            ["battery.meta", "battery.sav", "slot0.name", "slot0.snap"]
        );
        assert_eq!(mirror.namespaces(), ["ABCDEF0123", "borrowed-FF"]);
    }

    #[test]
    fn a_failed_write_tries_again_with_the_newest_bytes_and_keeps_local_storage() {
        let mut mirror = Mirror::default();
        mirror.adopt_local(filled_local_storage());
        let failed = mirror.take_pending();
        mirror.write(key(GAME, "battery.sav"), &[4]);
        mirror.delete(key(GAME, "slot0.name"));
        let written = mirror.take_pending();
        mirror.retry(failed);

        let retry = mirror.take_pending();
        assert_eq!(retry.moved.len(), 5);
        assert_eq!(retry.ops.len(), 5);
        assert!(
            retry
                .ops
                .contains(&Op::Put(key(GAME, "battery.sav"), vec![4]))
        );
        assert!(retry.ops.contains(&Op::Delete(key(GAME, "slot0.name"))));
        assert_eq!(written.ops.len(), 2);
        assert!(mirror.take_pending().is_empty());
    }

    #[test]
    fn local_storage_wins_over_an_older_copy_in_the_database() {
        let mut mirror = Mirror::default();
        mirror.adopt_local(filled_local_storage());
        mirror.fill([
            (key(GAME, "battery.sav"), vec![0xEE]),
            (key(GAME, "slot3.snap"), vec![3]),
        ]);
        assert_eq!(
            mirror.read(&key(GAME, "battery.sav")),
            Some(&[1, 2, 3, 4][..])
        );
        assert_eq!(mirror.read(&key(GAME, "slot3.snap")), Some(&[3][..]));
    }

    #[test]
    fn writes_and_deletes_before_the_database_answers_are_kept() {
        let mut mirror = Mirror::default();
        mirror.write(key(GAME, "battery.sav"), &[8]);
        mirror.delete(key(GAME, "slot0.snap"));
        mirror.fill([
            (key(GAME, "battery.sav"), vec![1]),
            (key(GAME, "slot0.snap"), vec![2]),
            (key(GAME, "slot0.meta"), vec![3]),
        ]);
        assert_eq!(mirror.read(&key(GAME, "battery.sav")), Some(&[8][..]));
        assert_eq!(mirror.read(&key(GAME, "slot0.snap")), None);
        assert_eq!(mirror.read(&key(GAME, "slot0.meta")), Some(&[3][..]));
        assert_eq!(
            mirror.take_pending().ops,
            [
                Op::Put(key(GAME, "battery.sav"), vec![8]),
                Op::Delete(key(GAME, "slot0.snap")),
            ]
        );
    }

    #[test]
    fn a_namespace_lists_only_its_own_files() {
        let mut mirror = Mirror::default();
        mirror.fill([
            ("AB/battery.sav".to_string(), vec![1]),
            ("AB-2/battery.sav".to_string(), vec![2]),
            ("ABC/slot0.snap".to_string(), vec![3]),
        ]);
        assert_eq!(mirror.list("AB"), ["battery.sav"]);
        assert_eq!(mirror.namespaces(), ["AB", "AB-2", "ABC"]);
    }

    #[test]
    fn the_crash_key_stays_out_of_the_files_and_goes_after_its_writes() {
        let mut mirror = Mirror::default();
        mirror.adopt_local([("citrine-crash".to_string(), encoded(&[1, 2]))]);
        assert!(mirror.namespaces().is_empty());
        mirror.write(key(GAME, "auto0.snap"), &[5]);
        let written = mirror.take_pending();
        mirror.remove_after_commit("citrine-crash".into());
        let after = mirror.take_pending();
        assert!(written.moved.is_empty());
        assert!(after.ops.is_empty());
        assert_eq!(after.moved, ["citrine-crash"]);
    }

    #[test]
    fn stats_walker_chats_and_dev_keys_stay_in_local_storage() {
        let mut mirror = Mirror::default();
        let kept = [
            "citrine/stats/stats.json",
            "citrine/walker/walker.bin",
            "citrine/chats/1234.bin",
            "citrine/retroachievements-token/token",
            "citrine/dev/route",
            "citrine/settings/settings.json",
        ];
        mirror.adopt_local(kept.map(|local| (local.to_string(), encoded(&[1]))));
        let batch = mirror.take_pending();
        assert!(batch.is_empty(), "{batch:?}");
        assert_eq!(mirror.namespaces(), Vec::<String>::new());
    }

    #[test]
    fn values_0_12_moved_into_the_database_go_back_once_and_never_over_newer_ones() {
        let mut local = BTreeMap::from([(
            "citrine/walker/walker.bin".to_string(),
            encoded(b"this session"),
        )]);
        let stored = vec![
            ("stats/stats.json".to_string(), b"old stats".to_vec()),
            ("walker/walker.bin".to_string(), b"older steps".to_vec()),
            ("chats/1234.bin".to_string(), b"chat".to_vec()),
            (
                "retroachievements-token/token".to_string(),
                b"token".to_vec(),
            ),
            (format!("{GAME}/battery.sav"), b"sram".to_vec()),
        ];
        let mut full = false;
        let mut mirror = Mirror::default();
        mirror.fill_and_recover(stored, |key, value| {
            if local.contains_key(key) {
                return true;
            }
            if full {
                return false;
            }
            local.insert(key.to_string(), value.to_string());
            full = key.starts_with("citrine/chats/");
            true
        });
        assert_eq!(local["citrine/stats/stats.json"], encoded(b"old stats"));
        assert_eq!(local["citrine/walker/walker.bin"], encoded(b"this session"));
        assert_eq!(local["citrine/chats/1234.bin"], encoded(b"chat"));
        assert!(!local.contains_key("citrine/retroachievements-token/token"));
        assert_eq!(
            mirror.read(&format!("{GAME}/battery.sav")),
            Some(&b"sram"[..])
        );
        assert_eq!(mirror.namespaces(), [GAME]);
        let batch = mirror.take_pending();
        assert_eq!(
            batch.ops,
            [
                Op::Delete("stats/stats.json".to_string()),
                Op::Delete("walker/walker.bin".to_string()),
                Op::Delete("chats/1234.bin".to_string()),
            ],
            "the token stays in the database until local storage has room"
        );
    }

    #[test]
    fn a_save_from_version_0_11_still_moves_out_of_local_storage() {
        let mut mirror = Mirror::default();
        mirror.adopt_local([(
            "citrine/ABCDEF0123/battery.sav".to_string(),
            "c3JhbQ==".to_string(),
        )]);
        assert_eq!(mirror.read("ABCDEF0123/battery.sav"), Some(&b"sram"[..]));
    }

    #[test]
    fn an_eight_megabyte_state_round_trips_through_the_database_only() {
        let big: Vec<u8> = (0..8 << 20).map(|i: u32| (i % 251) as u8).collect();
        assert!(!fits_local(&encoded(&big)));
        assert!(fits_local(&encoded(&[0; 4096])));

        let mut mirror = Mirror::default();
        let path = key(GAME, "slot0.snap");
        mirror.write(path.clone(), &big);
        let batch = mirror.take_pending();
        assert_eq!(batch.ops, [Op::Put(path.clone(), big.clone())]);
        mirror.retry(batch);
        assert_eq!(
            mirror.take_pending().ops,
            [Op::Put(path.clone(), big.clone())]
        );

        let mut reopened = Mirror::default();
        reopened.fill([(path.clone(), big.clone())]);
        assert_eq!(reopened.read(&path), Some(&big[..]));
    }
}

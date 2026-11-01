use super::Online;
use super::cloud::SyncState;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub type Values = BTreeMap<String, Value>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stamped {
    pub at: u64,
    pub value: Value,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub gone: bool,
}

impl Stamped {
    fn beats(&self, other: &Stamped) -> bool {
        if self.at != other.at {
            return self.at > other.at;
        }
        (self.gone, self.value.to_string()) > (other.gone, other.value.to_string())
    }

    fn holds(&self, value: &Value) -> bool {
        !self.gone && self.value == *value
    }
}

const SLOT: &str = "settings";

fn unsent() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncDoc {
    entries: BTreeMap<String, Stamped>,
    clock: u64,
    started: bool,
    #[serde(skip, default = "unsent")]
    unsent: bool,
}

impl SyncDoc {
    pub fn note(
        &mut self,
        current: &Values,
        defaults: &Values,
        known: impl Fn(&str) -> bool,
        now: u64,
    ) -> bool {
        if !self.started {
            self.started = true;
            for (key, value) in current {
                if self.entries.contains_key(key) {
                    continue;
                }
                let at = u64::from(defaults.get(key) != Some(value));
                self.entries.insert(
                    key.clone(),
                    Stamped {
                        at,
                        value: value.clone(),
                        gone: false,
                    },
                );
            }
            self.unsent = true;
            return true;
        }
        let mut stamp = None;
        let mut changed = false;
        for (key, value) in current {
            if self.entries.get(key).is_some_and(|e| e.holds(value)) {
                continue;
            }
            let at = *stamp.get_or_insert_with(|| self.tick(now));
            self.entries.insert(
                key.clone(),
                Stamped {
                    at,
                    value: value.clone(),
                    gone: false,
                },
            );
            changed = true;
        }
        let removed: Vec<String> = self
            .entries
            .iter()
            .filter(|(key, e)| !e.gone && !current.contains_key(*key) && known(key))
            .map(|(key, _)| key.clone())
            .collect();
        for key in removed {
            let at = *stamp.get_or_insert_with(|| self.tick(now));
            self.entries.insert(
                key,
                Stamped {
                    at,
                    value: Value::Null,
                    gone: true,
                },
            );
            changed = true;
        }
        self.unsent |= changed;
        changed
    }

    pub fn merge(&mut self, other: &SyncDoc) -> bool {
        let mut changed = false;
        for (key, theirs) in &other.entries {
            self.clock = self.clock.max(theirs.at);
            if self.entries.get(key).is_none_or(|mine| theirs.beats(mine)) {
                self.entries.insert(key.clone(), theirs.clone());
                changed = true;
            }
        }
        self.unsent |= changed;
        changed
    }

    pub fn values(&self) -> Values {
        self.entries
            .iter()
            .filter(|(_, e)| !e.gone)
            .map(|(key, e)| (key.clone(), e.value.clone()))
            .collect()
    }

    pub fn get(&self, key: &str) -> Option<&Stamped> {
        self.entries.get(key)
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(&self.entries).expect("settings always serialise")
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let entries = serde_json::from_slice(bytes).ok()?;
        Some(Self {
            entries,
            clock: 0,
            started: true,
            unsent: false,
        })
    }

    fn tick(&mut self, now: u64) -> u64 {
        self.clock = now.max(self.clock + 1);
        self.clock
    }
}

impl Online {
    pub fn sync_settings(&mut self, doc: &mut SyncDoc, on: bool) -> bool {
        if !on {
            self.account_stop(SLOT);
            doc.unsent = true;
            return false;
        }
        let mut changed = false;
        for copy in self.account_take(SLOT) {
            match SyncDoc::from_bytes(&copy.data) {
                Some(theirs) => changed |= doc.merge(&theirs),
                None => log::warn!("the settings from {} could not be read", copy.device),
            }
        }
        if doc.unsent {
            doc.unsent = false;
            self.account_put(SLOT, doc.to_bytes());
        }
        changed
    }

    pub fn settings_problem(&self) -> Option<SyncState> {
        self.account_state(SLOT).filter(|state| {
            matches!(
                state,
                SyncState::Full | SyncState::TooLarge | SyncState::NoStorage
            )
        })
    }
}

pub fn now_millis() -> u64 {
    web_time::SystemTime::now()
        .duration_since(web_time::SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn values(pairs: &[(&str, Value)]) -> Values {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    }

    fn device(pairs: &[(&str, Value)]) -> SyncDoc {
        let mut doc = SyncDoc::default();
        let defaults = values(&[("theme", json!("Mocha")), ("matrix", json!(false))]);
        doc.note(&values(pairs), &defaults, |_| true, 0);
        doc
    }

    fn change(doc: &mut SyncDoc, key: &str, value: Value, now: u64) {
        let mut current = doc.values();
        current.insert(key.to_owned(), value);
        doc.note(&current, &Values::new(), |_| true, now);
    }

    fn sync(a: &mut SyncDoc, b: &mut SyncDoc) {
        a.merge(b);
        b.merge(a);
    }

    #[test]
    fn a_fresh_device_takes_the_choices_of_a_used_one() {
        let mut used = device(&[("theme", json!("Latte")), ("matrix", json!(false))]);
        let mut fresh = device(&[("theme", json!("Mocha")), ("matrix", json!(false))]);
        sync(&mut fresh, &mut used);
        assert_eq!(fresh.values()["theme"], json!("Latte"));
        assert_eq!(used.values()["theme"], json!("Latte"));
    }

    #[test]
    fn interleaved_changes_on_two_devices_keep_the_last_per_setting() {
        let start = [("theme", json!("Mocha")), ("matrix", json!(false))];
        let mut desk = device(&start);
        let mut phone = device(&start);
        change(&mut desk, "theme", json!("Latte"), 1_000);
        change(&mut phone, "matrix", json!(true), 2_000);
        change(&mut phone, "theme", json!("Frappe"), 3_000);
        change(&mut desk, "theme", json!("Macchiato"), 4_000);
        sync(&mut desk, &mut phone);
        assert_eq!(desk.values(), phone.values());
        assert_eq!(desk.values()["theme"], json!("Macchiato"));
        assert_eq!(desk.values()["matrix"], json!(true));
    }

    #[test]
    fn a_change_made_after_seeing_another_wins_even_with_a_slow_clock() {
        let start = [("theme", json!("Mocha"))];
        let mut fast = device(&start);
        let mut slow = device(&start);
        change(&mut fast, "theme", json!("Latte"), 9_000);
        sync(&mut slow, &mut fast);
        change(&mut slow, "theme", json!("Frappe"), 100);
        sync(&mut slow, &mut fast);
        assert_eq!(fast.values()["theme"], json!("Frappe"));
    }

    #[test]
    fn same_moment_changes_settle_on_one_value_everywhere() {
        let start = [("theme", json!("Mocha"))];
        let mut a = device(&start);
        let mut b = device(&start);
        change(&mut a, "theme", json!("Latte"), 5_000);
        change(&mut b, "theme", json!("Frappe"), 5_000);
        let mut a2 = a.clone();
        let mut b2 = b.clone();
        sync(&mut a, &mut b);
        sync(&mut b2, &mut a2);
        assert_eq!(a.values(), b.values());
        assert_eq!(a.values(), a2.values());
    }

    #[test]
    fn a_deleted_palette_stays_deleted_and_a_new_one_on_the_other_device_stays() {
        let start = [("palettes/Old", json!([1])), ("theme", json!("Mocha"))];
        let mut desk = device(&start);
        let mut phone = device(&start);
        sync(&mut desk, &mut phone);
        let mut current = desk.values();
        current.remove("palettes/Old");
        desk.note(&current, &Values::new(), |_| true, 1_000);
        change(&mut phone, "palettes/New", json!([2]), 2_000);
        sync(&mut desk, &mut phone);
        assert_eq!(desk.values(), phone.values());
        assert!(!desk.values().contains_key("palettes/Old"));
        assert_eq!(desk.values()["palettes/New"], json!([2]));
    }

    #[test]
    fn settings_this_version_does_not_know_are_kept() {
        let mut newer = device(&[("theme", json!("Mocha"))]);
        change(&mut newer, "future", json!(7), 1_000);
        let mut older = device(&[("theme", json!("Mocha"))]);
        older.merge(&newer);
        let current = values(&[("theme", json!("Mocha"))]);
        older.note(&current, &Values::new(), |key| key != "future", 2_000);
        assert_eq!(older.values()["future"], json!(7));
    }

    #[test]
    fn an_unset_value_is_a_value_not_a_delete() {
        let mut desk = device(&[("custom", json!("Mine"))]);
        let mut phone = device(&[("custom", json!("Mine"))]);
        change(&mut desk, "custom", Value::Null, 1_000);
        sync(&mut desk, &mut phone);
        assert_eq!(phone.values().get("custom"), Some(&Value::Null));
    }

    #[test]
    fn the_blob_round_trips() {
        let mut doc = device(&[("theme", json!("Latte"))]);
        change(&mut doc, "theme", json!("Frappe"), 1_000);
        let back = SyncDoc::from_bytes(&doc.to_bytes()).unwrap();
        assert_eq!(back.values(), doc.values());
        assert!(SyncDoc::from_bytes(b"nope").is_none());
    }
}

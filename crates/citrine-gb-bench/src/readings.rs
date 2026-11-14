use crate::digest::Digest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Readings {
    #[serde(default)]
    pub(crate) meta: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) workloads: BTreeMap<String, Reading>,
    #[serde(default)]
    pub(crate) sizes: BTreeMap<String, u64>,
}

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Reading {
    #[serde(default)]
    pub(crate) console: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) wall_ms: Vec<f64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) ir: BTreeMap<String, Vec<u64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) peak_kb: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) digest: Option<Digest>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) failures: Vec<String>,
}

impl Readings {
    pub(crate) fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub(crate) fn save(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text + "\n").map_err(|e| format!("{}: {e}", path.display()))
    }

    pub(crate) fn absorb(&mut self, other: Readings) {
        for (key, value) in other.meta {
            self.meta.entry(key).or_insert(value);
        }
        self.sizes.extend(other.sizes);
        for (id, reading) in other.workloads {
            let mine = self.workloads.entry(id).or_default();
            if mine.console.is_empty() {
                mine.console = reading.console;
            }
            mine.wall_ms.extend(reading.wall_ms);
            for (profile, counts) in reading.ir {
                mine.ir.entry(profile).or_default().extend(counts);
            }
            mine.peak_kb = mine.peak_kb.max(reading.peak_kb);
            if mine.digest.is_none() {
                mine.digest = reading.digest;
            }
            mine.failures.extend(reading.failures);
        }
    }

    pub(crate) fn failed(&self) -> Vec<(&String, &Vec<String>)> {
        self.workloads
            .iter()
            .filter(|(_, reading)| !reading.failures.is_empty())
            .map(|(id, reading)| (id, &reading.failures))
            .collect()
    }
}

pub(crate) fn peak_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|rest| rest.trim().trim_end_matches("kB").trim().parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absorbing_appends_repeats_and_keeps_failures() {
        let mut a = Readings::default();
        a.workloads.entry("x".into()).or_default().wall_ms = vec![1.0];
        let mut b = Readings::default();
        let reading = b.workloads.entry("x".into()).or_default();
        reading.wall_ms = vec![2.0];
        reading.ir.insert("dist".into(), vec![5]);
        reading.failures.push("audio".into());
        a.absorb(b);
        let x = &a.workloads["x"];
        assert_eq!(x.wall_ms, vec![1.0, 2.0]);
        assert_eq!(x.ir["dist"], vec![5]);
        assert_eq!(a.failed().len(), 1);
    }
}

use crate::digest::Digest;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Platform {
    Gb,
    Gbc,
    Gba,
    Ds,
}

impl Platform {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Platform::Gb => "GB",
            Platform::Gbc => "GBC",
            Platform::Gba => "GBA",
            Platform::Ds => "DS",
        }
    }
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Kind {
    Solo,
    Pair,
    Link,
    Rollback,
    State,
    Ds3d,
    Ds2d,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
pub(crate) enum RomSource {
    Homebrew { homebrew: String },
    Ledger { ledger: String, path: String },
    File { file: String },
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct Workload {
    pub(crate) id: String,
    pub(crate) console: Platform,
    pub(crate) kind: Kind,
    pub(crate) rom: Option<RomSource>,
    pub(crate) frames: u32,
    #[serde(default)]
    pub(crate) seed: u64,
    #[serde(default)]
    pub(crate) depth: u32,
    #[serde(default)]
    pub(crate) script: Option<String>,
    #[serde(default)]
    pub(crate) scene: Option<String>,
}

#[derive(Deserialize)]
struct WorkloadsFile {
    workload: Vec<Workload>,
}

pub(crate) struct Manifest {
    pub(crate) workloads: Vec<Workload>,
    pub(crate) expected: BTreeMap<String, Digest>,
    pub(crate) dir: PathBuf,
}

impl Manifest {
    pub(crate) fn public() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    pub(crate) fn load(dir: &Path) -> Result<Self, String> {
        let path = dir.join("workloads.toml");
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let file: WorkloadsFile =
            toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let expected_path = dir.join("expected.toml");
        let expected = match std::fs::read_to_string(&expected_path) {
            Ok(text) => {
                toml::from_str(&text).map_err(|e| format!("{}: {e}", expected_path.display()))?
            }
            Err(_) => BTreeMap::new(),
        };
        let mut ids = std::collections::BTreeSet::new();
        for workload in &file.workload {
            if !ids.insert(&workload.id) {
                return Err(format!(
                    "{} appears twice in {}",
                    workload.id,
                    path.display()
                ));
            }
        }
        Ok(Self {
            workloads: file.workload,
            expected,
            dir: dir.to_path_buf(),
        })
    }

    pub(crate) fn save_expected(&self) -> Result<(), String> {
        let path = self.dir.join("expected.toml");
        let text = toml::to_string(&self.expected).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub(crate) fn find(&self, id: &str) -> Option<&Workload> {
        self.workloads.iter().find(|workload| workload.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_public_manifest_parses_and_every_workload_has_an_expected_digest() {
        let manifest = Manifest::load(&Manifest::public()).unwrap();
        assert!(!manifest.workloads.is_empty());
        for workload in &manifest.workloads {
            assert!(
                manifest.expected.contains_key(&workload.id),
                "{} has no expected digest",
                workload.id
            );
        }
    }
}

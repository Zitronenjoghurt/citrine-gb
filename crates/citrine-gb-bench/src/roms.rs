use crate::manifest::RomSource;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

#[path = "../../citrine-gb/tests/archives/mod.rs"]
mod archives;

#[derive(Deserialize)]
struct Ledger {
    sources: BTreeMap<String, LedgerSource>,
    suites: BTreeMap<String, Suite>,
}

#[derive(Deserialize)]
struct LedgerSource {
    #[serde(flatten)]
    source: archives::Source,
    #[serde(default)]
    built: bool,
}

#[derive(Deserialize)]
struct Suite {
    source: String,
    root: String,
    roms: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    path: String,
    sha256: String,
}

pub(crate) fn homebrew_dir() -> std::path::PathBuf {
    archives::repo_root()
        .join("crates")
        .join("citrine-gb-app")
        .join("homebrew")
}

pub(crate) fn load(source: &RomSource, dir: &Path) -> Result<Vec<u8>, String> {
    match source {
        RomSource::Homebrew { homebrew } => read_file(&homebrew_dir().join(homebrew)),
        RomSource::File { file } => read_file(&dir.join(file)),
        RomSource::Ledger { ledger, path } => from_ledger(ledger, path),
    }
}

fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if path.extension().is_some_and(|e| e == "br") {
        let mut out = Vec::new();
        brotli::Decompressor::new(data.as_slice(), 4096)
            .read_to_end(&mut out)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(out)
    } else {
        Ok(data)
    }
}

fn from_ledger(suite_name: &str, path: &str) -> Result<Vec<u8>, String> {
    let ledger: Ledger = archives::read_manifest()?;
    let suite = ledger
        .suites
        .get(suite_name)
        .ok_or_else(|| format!("no ledger suite {suite_name}"))?;
    let entry = suite
        .roms
        .iter()
        .find(|entry| entry.path == path)
        .ok_or_else(|| format!("no {path} in ledger suite {suite_name}"))?;
    let source = ledger
        .sources
        .get(&suite.source)
        .ok_or_else(|| format!("no ledger source {}", suite.source))?;
    let mut archive = if source.built {
        open_built(&source.source)?
    } else {
        archives::fetch(&suite.source, &source.source)?
    };
    let data = archives::read_entry(&mut archive, &format!("{}/{path}", suite.root))?;
    let actual = archives::sha256_hex(&data);
    if actual != entry.sha256 {
        return Err(format!(
            "{path} has SHA-256 {actual}, the ledger pins {}",
            entry.sha256
        ));
    }
    Ok(data)
}

fn open_built(source: &archives::Source) -> Result<zip::ZipArchive<std::fs::File>, String> {
    let path = archives::cache_dir().join(format!("built-{}.zip", source.sha256));
    let file = std::fs::File::open(&path)
        .map_err(|e| format!("{}: {e}, `make ledger-nds` builds it", path.display()))?;
    zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", path.display()))
}

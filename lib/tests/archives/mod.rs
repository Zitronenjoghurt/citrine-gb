use serde::Deserialize;
use serde::de::DeserializeOwned;
use sha2::Digest;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::process::Command;

#[derive(Deserialize)]
pub struct Source {
    pub url: String,
    pub sha256: String,
}

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

pub fn ledger_dir() -> PathBuf {
    repo_root().join("tests").join("ledger")
}

pub fn read_manifest<T: DeserializeOwned>() -> Result<T, String> {
    let path = ledger_dir().join("manifest.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("LEDGER_CACHE") {
        return PathBuf::from(dir);
    }
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(|| repo_root().join("target"));
    base.join("citrine-gb").join("ledger")
}

pub fn sha256_hex(data: &[u8]) -> String {
    sha2::Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn fetch(name: &str, source: &Source) -> Result<zip::ZipArchive<File>, String> {
    let dir = cache_dir();
    let path = dir.join(format!("{}.zip", source.sha256));
    if !path.is_file() {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let partial = dir.join(format!("{}.{}.part", source.sha256, std::process::id()));
        println!("fetching {name} from {}", source.url);
        let status = Command::new("curl")
            .args(["-fsSL", "--retry", "3", "-o"])
            .arg(&partial)
            .arg(&source.url)
            .status()
            .map_err(|e| format!("could not run curl: {e}"))?;
        if !status.success() {
            std::fs::remove_file(&partial).ok();
            return Err(format!("curl failed for {}", source.url));
        }
        let data = std::fs::read(&partial).map_err(|e| format!("{}: {e}", partial.display()))?;
        let actual = sha256_hex(&data);
        if actual != source.sha256 {
            std::fs::remove_file(&partial).ok();
            return Err(format!(
                "{name} has SHA-256 {actual}, the manifest pins {}",
                source.sha256
            ));
        }
        std::fs::rename(&partial, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let file = File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn read_entry(archive: &mut zip::ZipArchive<File>, path: &str) -> Result<Vec<u8>, String> {
    let mut entry = archive
        .by_name(path)
        .map_err(|e| format!("{path} in archive: {e}"))?;
    let mut data = Vec::new();
    entry
        .read_to_end(&mut data)
        .map_err(|e| format!("{path} in archive: {e}"))?;
    Ok(data)
}

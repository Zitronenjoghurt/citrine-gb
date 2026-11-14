use serde::Deserialize;
use serde::de::DeserializeOwned;
use sha2::Digest;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

static PARTS: AtomicU32 = AtomicU32::new(0);

#[derive(Deserialize)]
pub struct Source {
    pub url: String,
    pub sha256: String,
}

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
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

pub const RETRY_DELAYS: [Duration; 4] = [
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(30),
    Duration::from_secs(60),
];

pub fn fetch(name: &str, source: &Source) -> Result<zip::ZipArchive<File>, String> {
    let mut urls = vec![source.url.clone()];
    urls.extend(codeload_url(&source.url));
    let path = download(&cache_dir(), name, &source.sha256, &urls, &RETRY_DELAYS)?;
    let file = File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn codeload_url(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://github.com/")?;
    let (repo, commit) = rest.split_once("/archive/")?;
    let commit = commit.strip_suffix(".zip")?;
    Some(format!("https://codeload.github.com/{repo}/zip/{commit}"))
}

pub fn download(
    dir: &Path,
    name: &str,
    sha256: &str,
    urls: &[String],
    delays: &[Duration],
) -> Result<PathBuf, String> {
    let path = dir.join(format!("{sha256}.zip"));
    if path.is_file() {
        return Ok(path);
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut failures = Vec::new();
    for url in urls {
        println!("fetching {name} from {url}");
        let partial = dir.join(format!(
            "{sha256}.{}.{}.part",
            std::process::id(),
            PARTS.fetch_add(1, Ordering::Relaxed)
        ));
        let outcome = download_with_retries(url, &partial, delays);
        let checked = outcome.and_then(|()| {
            let data =
                std::fs::read(&partial).map_err(|e| format!("{}: {e}", partial.display()))?;
            let actual = sha256_hex(&data);
            if actual == sha256 {
                Ok(())
            } else {
                Err(format!("got SHA-256 {actual}"))
            }
        });
        match checked {
            Ok(()) => {
                std::fs::rename(&partial, &path).map_err(|e| format!("{}: {e}", path.display()))?;
                return Ok(path);
            }
            Err(failure) => {
                std::fs::remove_file(&partial).ok();
                failures.push(format!("{url} ({failure})"));
            }
        }
    }
    Err(format!(
        "{name}: could not download SHA-256 {sha256} from {}",
        failures.join(", ")
    ))
}

enum Attempt {
    Done,
    Transient(String),
    Fatal(String),
}

fn download_with_retries(url: &str, partial: &Path, delays: &[Duration]) -> Result<(), String> {
    let mut waits = delays.iter();
    loop {
        match attempt(url, partial) {
            Attempt::Done => return Ok(()),
            Attempt::Fatal(failure) => return Err(failure),
            Attempt::Transient(failure) => match waits.next() {
                Some(wait) => {
                    println!("{url}: {failure}, trying again in {} s", wait.as_secs());
                    std::thread::sleep(*wait);
                }
                None => return Err(format!("{failure}, {} tries", delays.len() + 1)),
            },
        }
    }
}

fn attempt(url: &str, partial: &Path) -> Attempt {
    std::fs::remove_file(partial).ok();
    let output = Command::new("curl")
        .args([
            "-sSL",
            "--connect-timeout",
            "30",
            "--speed-limit",
            "1024",
            "--speed-time",
            "60",
            "-w",
            "%{http_code}",
            "-o",
        ])
        .arg(partial)
        .arg(url)
        .output();
    let output = match output {
        Ok(output) => output,
        Err(e) => return Attempt::Fatal(format!("could not run curl: {e}")),
    };
    let code: u16 = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap_or(0);
    match (output.status.code(), code) {
        (Some(0), 200) => Attempt::Done,
        (Some(0), 408 | 429 | 500..=599) => Attempt::Transient(format!("HTTP {code}")),
        (Some(0), _) => Attempt::Fatal(format!("HTTP {code}")),
        (Some(exit @ (5 | 6 | 7 | 16 | 18 | 28 | 35 | 52 | 55 | 56 | 92)), _) => {
            Attempt::Transient(format!("curl exit {exit}"))
        }
        (exit, _) => Attempt::Fatal(format!(
            "curl exit {}",
            exit.map_or("by signal".into(), |exit| exit.to_string())
        )),
    }
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

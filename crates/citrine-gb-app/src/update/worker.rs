use super::release::{Asset, Platform, Release, SIGNATURE, SUMS, sum_for};
use super::{Offer, Phase, Shared, Updater};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const LATEST: &str = "https://api.github.com/repos/Zitronenjoghurt/citrine-gb/releases/latest";
pub(super) const PUBLIC_KEY: Option<&str> =
    Some("RWQyu2jjyDb5HH9IZipViEUJlb8/75wbJodYRaKuXofgthoC/vApL6PN");
const DAY: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_DOWNLOAD: u64 = 256 * 1024 * 1024;

pub type Fetch =
    Arc<dyn Fn(&str, &mut dyn FnMut(u64, u64)) -> anyhow::Result<Vec<u8>> + Send + Sync>;
pub type Install = Arc<dyn Fn(&[u8], &Asset) -> anyhow::Result<()> + Send + Sync>;

#[derive(Clone)]
pub struct Backend {
    pub platform: Platform,
    pub current: String,
    pub key: Option<String>,
    pub latest: String,
    pub fetch: Fetch,
    pub install: Install,
}

impl Backend {
    pub fn live(platform: Platform) -> Self {
        Self {
            platform,
            current: env!("CARGO_PKG_VERSION").to_owned(),
            key: PUBLIC_KEY.map(str::to_owned),
            latest: LATEST.to_owned(),
            fetch: Arc::new(fetch),
            install: super::install::live(),
        }
    }
}

fn fetch(url: &str, progress: &mut dyn FnMut(u64, u64)) -> anyhow::Result<Vec<u8>> {
    let mut response = ureq::get(url)
        .header("User-Agent", concat!("Citrine/", env!("CARGO_PKG_VERSION")))
        .call()?;
    let total = response.body().content_length().unwrap_or(0);
    let mut reader = response
        .body_mut()
        .with_config()
        .limit(MAX_DOWNLOAD)
        .reader();
    let mut bytes = Vec::with_capacity(total.min(MAX_DOWNLOAD) as usize);
    let mut chunk = vec![0; 64 * 1024];
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
        progress(bytes.len() as u64, total);
    }
    Ok(bytes)
}

pub fn verify(key: &str, data: &[u8], signature: &[u8]) -> bool {
    let key = key.trim();
    let key = if key.contains('\n') {
        minisign_verify::PublicKey::decode(key)
    } else {
        minisign_verify::PublicKey::from_base64(key)
    };
    let Ok(key) = key else {
        return false;
    };
    let Ok(signature) = std::str::from_utf8(signature) else {
        return false;
    };
    let Ok(signature) = minisign_verify::Signature::decode(signature) else {
        return false;
    };
    key.verify(data, &signature, false).is_ok()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn offer(backend: &Backend, release: &Release) -> Offer {
    Offer {
        version: release.tag_name.trim_start_matches('v').to_owned(),
        page: release.html_url.clone(),
        installable: backend.key.is_some()
            && release.signed_download_for(backend.platform).is_some(),
        size: release
            .download_for(backend.platform)
            .map_or(0, |asset| asset.size),
    }
}

pub struct Failure {
    pub reason: String,
    pub retry: bool,
}

fn refuse(reason: &str) -> Failure {
    Failure {
        reason: reason.to_owned(),
        retry: false,
    }
}

fn network(err: anyhow::Error) -> Failure {
    log::warn!("update download failed: {err:#}");
    Failure {
        reason: "The download failed".to_owned(),
        retry: true,
    }
}

pub fn fetch_verified(
    backend: &Backend,
    release: &Release,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(Vec<u8>, Asset), Failure> {
    let asset = release
        .signed_download_for(backend.platform)
        .ok_or_else(|| refuse("This release has no signed download for your system."))?;
    let key = backend
        .key
        .as_deref()
        .ok_or_else(|| refuse("This copy of Citrine has no key to check updates with."))?;
    let get = |name: &str| -> Result<Vec<u8>, Failure> {
        let asset = release
            .asset(name)
            .ok_or_else(|| refuse("The release is missing its checksums."))?;
        (backend.fetch)(&asset.browser_download_url, &mut |_, _| {}).map_err(network)
    };
    let sums = get(SUMS)?;
    let signature = get(SIGNATURE)?;
    if !verify(key, &sums, &signature) {
        return Err(refuse("The signature doesn't match"));
    }
    let want = sum_for(&String::from_utf8_lossy(&sums), &asset.name)
        .ok_or_else(|| refuse("Not in the signed list"))?;
    let size = asset.size;
    let bytes = (backend.fetch)(&asset.browser_download_url, &mut |done, total| {
        progress(done, if total > 0 { total } else { size })
    })
    .map_err(network)?;
    if hex(&Sha256::digest(&bytes)) != want {
        return Err(refuse("The checksum doesn't match"));
    }
    Ok((bytes, asset.clone()))
}

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) -> bool {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(work)
        .is_ok()
}

impl Updater {
    pub fn with_backend(backend: Backend) -> Self {
        Self {
            backend: Some(backend),
            ..Self::default()
        }
    }

    pub fn tick(&mut self, enabled: bool) {
        let Some(backend) = self.backend.clone().filter(|_| enabled) else {
            return;
        };
        if self.last_check.is_some_and(|last| last.elapsed() < DAY) {
            return;
        }
        {
            let mut shared = self.lock();
            if shared.checking || !matches!(shared.phase, Phase::Idle | Phase::Available(_)) {
                return;
            }
            shared.checking = true;
        }
        self.last_check = Some(Instant::now());
        let shared = self.shared.clone();
        let started = spawn("update check", move || check(&backend, &shared));
        if !started {
            self.lock().checking = false;
        }
    }

    pub(super) fn download(&mut self, offer: Offer) {
        let Some(backend) = self.backend.clone() else {
            return;
        };
        let release = {
            let mut shared = self.lock();
            let Some(release) = shared.release.clone() else {
                return;
            };
            shared.phase = Phase::Downloading {
                offer: offer.clone(),
                done: 0,
                total: release
                    .download_for(backend.platform)
                    .map_or(0, |asset| asset.size),
            };
            release
        };
        let shared = self.shared.clone();
        let failed = offer.clone();
        let started = spawn("update download", move || {
            let phase = match install(&backend, &release, &shared) {
                Ok(()) => Phase::Ready(offer),
                Err(Failure { reason, retry }) => Phase::Failed {
                    offer,
                    reason,
                    retry,
                },
            };
            shared.lock().unwrap_or_else(|e| e.into_inner()).phase = phase;
        });
        if !started {
            self.lock().phase = Phase::Failed {
                offer: failed,
                reason: "The download didn't start".into(),
                retry: true,
            };
        }
    }

    #[cfg(test)]
    pub fn wait(&self) {
        while {
            let shared = self.lock();
            shared.checking || matches!(shared.phase, Phase::Downloading { .. })
        } {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

fn check(backend: &Backend, shared: &Mutex<Shared>) {
    let found = (backend.fetch)(&backend.latest, &mut |_, _| {})
        .ok()
        .and_then(|json| Release::parse(&json));
    let mut shared = shared.lock().unwrap_or_else(|e| e.into_inner());
    shared.checking = false;
    let Some(release) = found else {
        return;
    };
    if matches!(shared.phase, Phase::Idle | Phase::Available(_)) {
        shared.phase = if release.newer_than(&backend.current) {
            Phase::Available(offer(backend, &release))
        } else {
            Phase::Idle
        };
    }
    shared.release = Some(release);
}

fn install(backend: &Backend, release: &Release, shared: &Mutex<Shared>) -> Result<(), Failure> {
    let mut progress = |done: u64, total: u64| {
        let mut shared = shared.lock().unwrap_or_else(|e| e.into_inner());
        if let Phase::Downloading {
            done: now,
            total: all,
            ..
        } = &mut shared.phase
        {
            *now = done;
            *all = total.max(done);
        }
    };
    let (bytes, asset) = fetch_verified(backend, release, &mut progress)?;
    (backend.install)(&bytes, &asset).map_err(|err| refuse(&err.to_string()))
}

#![expect(
    clippy::disallowed_types,
    reason = "the update job and the Updater both write the phase, and citrine-gb-threads has no type for two writers yet"
)]

#[cfg(not(target_arch = "wasm32"))]
mod install;
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
mod release;
#[cfg(not(target_arch = "wasm32"))]
mod worker;

#[cfg(not(target_arch = "wasm32"))]
pub use install::relaunch;
#[cfg(not(target_arch = "wasm32"))]
use release::Platform;
use release::Release;

use std::sync::{Arc, Mutex};

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub version: String,
    pub page: String,
    pub installable: bool,
    pub size: u64,
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    Available(Offer),
    Downloading {
        offer: Offer,
        done: u64,
        total: u64,
    },
    Ready(Offer),
    Failed {
        offer: Offer,
        reason: String,
        retry: bool,
    },
}

impl Phase {
    pub fn offer(&self) -> Option<&Offer> {
        match self {
            Self::Idle => None,
            Self::Available(offer) | Self::Ready(offer) => Some(offer),
            Self::Downloading { offer, .. } | Self::Failed { offer, .. } => Some(offer),
        }
    }

    pub fn waiting(&self) -> bool {
        self.offer().is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tap {
    Nothing,
    Open(String),
    Restart,
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
#[derive(Default)]
struct Shared {
    phase: Phase,
    release: Option<Release>,
    checking: bool,
}

#[cfg_attr(target_arch = "wasm32", derive(Default))]
pub struct Updater {
    shared: Arc<Mutex<Shared>>,
    restart: bool,
    #[cfg(not(target_arch = "wasm32"))]
    backend: Option<worker::Backend>,
    #[cfg(not(target_arch = "wasm32"))]
    last_check: Option<std::time::Instant>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for Updater {
    fn default() -> Self {
        Self {
            shared: Arc::default(),
            restart: false,
            #[cfg(not(target_arch = "wasm32"))]
            backend: Platform::current().map(worker::Backend::live),
            #[cfg(not(target_arch = "wasm32"))]
            last_check: None,
        }
    }
}

impl Updater {
    pub fn phase(&self) -> Phase {
        self.lock().phase.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn take_restart(&mut self) -> bool {
        std::mem::take(&mut self.restart)
    }

    pub fn tap(&mut self, spawner: &citrine_gb_threads::Spawner) -> Tap {
        let phase = self.phase();
        let Some(offer) = phase.offer().cloned() else {
            return Tap::Nothing;
        };
        match phase {
            Phase::Downloading { .. } | Phase::Idle => Tap::Nothing,
            Phase::Ready(_) if cfg!(target_os = "android") => {
                self.reopen_installer();
                Tap::Nothing
            }
            Phase::Ready(_) => {
                self.restart = true;
                Tap::Restart
            }
            Phase::Available(_) | Phase::Failed { retry: true, .. } if offer.installable => {
                self.download(spawner, offer);
                Tap::Nothing
            }
            Phase::Available(_) | Phase::Failed { .. } => Tap::Open(offer.page),
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn tick(&mut self, _spawner: &citrine_gb_threads::Spawner, _enabled: bool) {}

    #[cfg(target_arch = "wasm32")]
    fn download(&mut self, _spawner: &citrine_gb_threads::Spawner, _offer: Offer) {}

    #[cfg(not(target_os = "android"))]
    fn reopen_installer(&self) {}

    #[cfg(target_os = "android")]
    fn reopen_installer(&self) {
        install::reopen();
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::release::{Platform, Release};
    use super::worker::Backend;
    use super::{Phase, Tap, Updater};
    use sha2::Digest;
    use sha2::Sha256;
    use std::collections::HashMap;
    use std::io::Cursor;
    use std::sync::{Arc, Mutex};

    pub(super) const TAG: &str = "v0.11.0";

    pub(super) fn keys() -> minisign::KeyPair {
        minisign::KeyPair::generate_unencrypted_keypair().unwrap()
    }

    pub(super) fn sign(keys: &minisign::KeyPair, data: &[u8]) -> Vec<u8> {
        minisign::sign(Some(&keys.pk), &keys.sk, Cursor::new(data), Some(TAG), None)
            .unwrap()
            .to_string()
            .into_bytes()
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    pub(super) fn tar_gz(dir: &str, name: &str, binary: &[u8]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        for (path, data) in [
            (format!("{dir}/LICENSE"), &b"licence"[..]),
            (format!("{dir}/{name}"), binary),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, path, data).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    pub(super) struct Server {
        files: HashMap<String, Vec<u8>>,
    }

    pub(super) const BASE: &str = "https://example.test/";

    impl Server {
        pub(super) fn release(platform: Platform, names: &[&str]) -> Release {
            Release::parse(Self::release_json(platform, names).as_bytes()).unwrap()
        }

        pub(super) fn release_json(platform: Platform, names: &[&str]) -> String {
            let asset = |name: &str| {
                format!(r#"{{"name":"{name}","size":7,"browser_download_url":"{BASE}{name}"}}"#)
            };
            let mut assets: Vec<String> = names.iter().map(|name| asset(name)).collect();
            assets.push(asset(&platform.asset_name(TAG)));
            format!(
                r#"{{"tag_name":"{TAG}","html_url":"https://github.com/Zitronenjoghurt/citrine-gb/releases/tag/{TAG}","assets":[{}]}}"#,
                assets.join(",")
            )
        }

        pub(super) fn new(
            release_names: &[&str],
            download: &[u8],
            sums: &[u8],
            signature: Option<Vec<u8>>,
        ) -> Self {
            let platform = Platform::LinuxX86_64;
            let mut files = HashMap::new();
            files.insert(
                "latest".to_owned(),
                Self::release_json(platform, release_names).into_bytes(),
            );
            files.insert(platform.asset_name(TAG), download.to_vec());
            files.insert("SHA256SUMS.txt".to_owned(), sums.to_vec());
            if let Some(signature) = signature {
                files.insert("SHA256SUMS.txt.minisig".to_owned(), signature);
            }
            Self { files }
        }

        pub(super) fn updater(
            self,
            key: Option<String>,
            installed: Arc<Mutex<Vec<Vec<u8>>>>,
        ) -> Updater {
            let files = Arc::new(self.files);
            Updater::with_backend(Backend {
                platform: Platform::LinuxX86_64,
                current: "0.10.0".into(),
                key,
                latest: format!("{BASE}latest"),
                fetch: Arc::new(move |url, progress| {
                    let name = url.strip_prefix(BASE).unwrap();
                    let bytes = files
                        .get(name)
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("404 {name}"))?;
                    progress(bytes.len() as u64, bytes.len() as u64);
                    Ok(bytes)
                }),
                install: Arc::new(move |bytes, _| {
                    installed.lock().unwrap().push(bytes.to_vec());
                    Ok(())
                }),
            })
        }
    }

    fn sums_for(download: &[u8]) -> Vec<u8> {
        format!(
            "{}  {}\n{}  citrine-{TAG}-macos.dmg\n",
            hex(&Sha256::digest(download)),
            Platform::LinuxX86_64.asset_name(TAG),
            hex(&Sha256::digest(b"other")),
        )
        .into_bytes()
    }

    pub(super) const SIGNED: &[&str] = &["SHA256SUMS.txt", "SHA256SUMS.txt.minisig"];

    fn run(updater: &mut Updater) -> Phase {
        updater.tick(&spawner(), true);
        updater.wait();
        if matches!(updater.phase(), Phase::Available(_)) {
            updater.tap(&spawner());
            updater.wait();
        }
        updater.phase()
    }

    #[test]
    fn a_good_signature_installs() {
        let keys = keys();
        let download = tar_gz("citrine-v0.11.0-linux-x86_64", "citrine", b"new binary");
        let sums = sums_for(&download);
        let server = Server::new(SIGNED, &download, &sums, Some(sign(&keys, &sums)));
        let installed = Arc::default();
        let mut updater = server.updater(Some(keys.pk.to_base64()), Arc::clone(&installed));
        let phase = run(&mut updater);
        assert!(matches!(phase, Phase::Ready(_)), "{phase:?}");
        assert_eq!(installed.lock().unwrap().as_slice(), [download]);
        assert_eq!(updater.tap(&spawner()), Tap::Restart);
        assert!(updater.take_restart());
    }

    fn refused(updater: &mut Updater, reason: &str, installed: &Mutex<Vec<Vec<u8>>>) {
        match run(updater) {
            Phase::Failed {
                reason: got, retry, ..
            } => {
                assert!(got.contains(reason), "{got}");
                assert!(!retry);
            }
            other => panic!("{other:?}"),
        }
        assert!(installed.lock().unwrap().is_empty());
        assert!(matches!(updater.tap(&spawner()), Tap::Open(_)));
        updater.wait();
        assert!(installed.lock().unwrap().is_empty());
    }

    #[test]
    fn a_signature_from_another_key_refuses() {
        let download = b"payload".to_vec();
        let sums = sums_for(&download);
        let server = Server::new(SIGNED, &download, &sums, Some(sign(&keys(), &sums)));
        let installed = Arc::default();
        let mut updater = server.updater(Some(keys().pk.to_base64()), Arc::clone(&installed));
        refused(&mut updater, "signature", &installed);
    }

    #[test]
    fn checksums_changed_after_signing_refuse() {
        let keys = keys();
        let download = b"payload".to_vec();
        let sums = sums_for(&download);
        let signature = sign(&keys, &sums_for(b"what was signed"));
        let server = Server::new(SIGNED, &download, &sums, Some(signature));
        let installed = Arc::default();
        let mut updater = server.updater(Some(keys.pk.to_base64()), Arc::clone(&installed));
        refused(&mut updater, "signature", &installed);
    }

    #[test]
    fn a_download_that_differs_from_its_signed_checksum_refuses() {
        let keys = keys();
        let sums = sums_for(b"the real build");
        let server = Server::new(SIGNED, b"something else", &sums, Some(sign(&keys, &sums)));
        let installed = Arc::default();
        let mut updater = server.updater(Some(keys.pk.to_base64()), Arc::clone(&installed));
        refused(&mut updater, "checksum", &installed);
    }

    #[test]
    fn a_dropped_download_offers_another_try() {
        let keys = keys();
        let download = b"payload".to_vec();
        let sums = sums_for(&download);
        let mut server = Server::new(SIGNED, &download, &sums, Some(sign(&keys, &sums)));
        server.files.remove(&Platform::LinuxX86_64.asset_name(TAG));
        let installed = Arc::<Mutex<Vec<Vec<u8>>>>::default();
        let mut updater = server.updater(Some(keys.pk.to_base64()), Arc::clone(&installed));
        let phase = run(&mut updater);
        assert!(
            matches!(phase, Phase::Failed { retry: true, .. }),
            "{phase:?}"
        );
        assert_eq!(updater.tap(&spawner()), Tap::Nothing);
        updater.wait();
        assert!(matches!(updater.phase(), Phase::Failed { retry: true, .. }));
        assert!(installed.lock().unwrap().is_empty());
    }

    #[test]
    fn a_missing_signature_only_links_the_release_page() {
        let keys = keys();
        let download = b"payload".to_vec();
        let sums = sums_for(&download);
        let server = Server::new(&["SHA256SUMS.txt"], &download, &sums, None);
        let installed = Arc::<Mutex<Vec<Vec<u8>>>>::default();
        let mut updater = server.updater(Some(keys.pk.to_base64()), Arc::clone(&installed));
        updater.tick(&spawner(), true);
        updater.wait();
        let Phase::Available(offer) = updater.phase() else {
            panic!("{:?}", updater.phase());
        };
        assert!(!offer.installable);
        assert_eq!(offer.version, "0.11.0");
        assert_eq!(updater.tap(&spawner()), Tap::Open(offer.page));
        updater.wait();
        assert!(installed.lock().unwrap().is_empty());
    }

    #[test]
    fn a_build_without_a_key_only_links_the_release_page() {
        let keys = keys();
        let download = b"payload".to_vec();
        let sums = sums_for(&download);
        let server = Server::new(SIGNED, &download, &sums, Some(sign(&keys, &sums)));
        let installed = Arc::<Mutex<Vec<Vec<u8>>>>::default();
        let mut updater = server.updater(None, Arc::clone(&installed));
        updater.tick(&spawner(), true);
        updater.wait();
        let Phase::Available(offer) = updater.phase() else {
            panic!("{:?}", updater.phase());
        };
        assert!(!offer.installable);
        assert!(matches!(updater.tap(&spawner()), Tap::Open(_)));
    }

    #[test]
    fn the_switch_off_means_no_check() {
        let server = Server::new(SIGNED, b"x", b"", None);
        let mut updater = server.updater(None, Arc::default());
        updater.tick(&spawner(), false);
        updater.wait();
        assert_eq!(updater.phase(), Phase::Idle);
    }

    fn spawner() -> citrine_gb_threads::Spawner {
        citrine_gb_threads::Threads::new(citrine_gb_threads::Mode::Threads).spawner()
    }
}

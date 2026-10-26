use openh264::OpenH264API;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const LICENSE: &str = include_str!("openh264-binary-license.txt");
const SERVER: &str = "https://ciscobinary.openh264.org/";
const MAX_DOWNLOAD: u64 = 8 * 1024 * 1024;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Binary {
    pub file: &'static str,
    pub sha256: &'static str,
}

const MAC_ARM64: Binary = Binary {
    file: "libopenh264-2.7.0-mac-arm64.dylib",
    sha256: "2e1ae3cd10c08328ca03a1f304fd2a1a2a7e5658c6d5a8582101a963f7995ef0",
};
const MAC_X64: Binary = Binary {
    file: "libopenh264-2.7.0-mac-x64.dylib",
    sha256: "31d38bcbe9348e9b44ca38dd08fa1e2af79ffce6521276449ef05e3cd39136f4",
};
const WINDOWS_X64: Binary = Binary {
    file: "openh264-2.7.0-win64.dll",
    sha256: "0ef5e7f3b38753b15c13afa0418d4d7805fa228e6adfd3e425bbf758cff1c288",
};
const LINUX_X64: Binary = Binary {
    file: "libopenh264-2.7.0-linux64.8.so",
    sha256: "1522f873a482923c5ec018a4415e1866898197528828c50f90fd300c94620df3",
};
const LINUX_ARM64: Binary = Binary {
    file: "libopenh264-2.7.0-linux-arm64.8.so",
    sha256: "9ac664b0154fd0868491c2a904397b8a05779874c0603b8b1f16ad15de1ffd82",
};

pub fn binary_for(os: &str, arch: &str) -> Option<Binary> {
    match (os, arch) {
        ("macos", "aarch64") => Some(MAC_ARM64),
        ("macos", "x86_64") => Some(MAC_X64),
        ("windows", "x86_64") => Some(WINDOWS_X64),
        ("linux", "x86_64") => Some(LINUX_X64),
        ("linux", "aarch64") => Some(LINUX_ARM64),
        _ => None,
    }
}

impl Binary {
    pub fn url(&self) -> String {
        format!("{SERVER}{}.bz2", self.file)
    }

    fn path_in(&self, dir: &Path) -> PathBuf {
        dir.join(self.file)
    }

    fn matches(&self, bytes: &[u8]) -> bool {
        hex(&Sha256::digest(bytes)) == self.sha256
    }

    pub fn unpack(&self, compressed: &[u8]) -> anyhow::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        bzip2::read::BzDecoder::new(compressed)
            .take(MAX_DOWNLOAD * 4)
            .read_to_end(&mut bytes)
            .map_err(|_| anyhow::anyhow!("the download was damaged"))?;
        if !self.matches(&bytes) {
            anyhow::bail!("the download didn't match Cisco's published checksum");
        }
        Ok(bytes)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub type Fetch = Box<dyn FnOnce(&str) -> anyhow::Result<Vec<u8>> + Send>;

pub fn from_cisco() -> Fetch {
    Box::new(|url| {
        let bytes = ureq::get(url)
            .call()?
            .body_mut()
            .with_config()
            .limit(MAX_DOWNLOAD)
            .read_to_vec()?;
        Ok(bytes)
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Status {
    Ready,
    Missing,
    Downloading,
    Unsupported,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Answer {
    Downloading,
    Declined,
}

#[derive(Default)]
struct State {
    downloading: bool,
    verified: Option<bool>,
    failure: Option<String>,
}

pub struct Codec {
    dir: PathBuf,
    binary: Option<Binary>,
    state: Arc<Mutex<State>>,
}

impl Codec {
    pub fn new(dir: PathBuf, binary: Option<Binary>) -> Self {
        Self {
            dir,
            binary,
            state: Arc::default(),
        }
    }

    pub fn status(&self) -> Status {
        let Some(binary) = self.binary else {
            return Status::Unsupported;
        };
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.downloading {
            return Status::Downloading;
        }
        let verified = *state.verified.get_or_insert_with(|| {
            std::fs::read(binary.path_in(&self.dir)).is_ok_and(|bytes| binary.matches(&bytes))
        });
        if verified {
            Status::Ready
        } else {
            Status::Missing
        }
    }

    pub fn answer(&self, download: bool, fetch: Fetch) -> Answer {
        if !download {
            return Answer::Declined;
        }
        self.download(fetch);
        Answer::Downloading
    }

    pub fn download(&self, fetch: Fetch) {
        let Some(binary) = self.binary else {
            return;
        };
        {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.downloading {
                return;
            }
            state.downloading = true;
            state.failure = None;
        }
        let dir = self.dir.clone();
        let state = self.state.clone();
        let spawned = std::thread::Builder::new()
            .name("codec download".into())
            .spawn(move || {
                let result = install(binary, &dir, fetch);
                let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
                state.downloading = false;
                state.verified = Some(result.is_ok());
                state.failure = result.err().map(|err| err.to_string());
            });
        if let Err(err) = spawned {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.downloading = false;
            state.failure = Some(err.to_string());
        }
    }

    #[cfg(test)]
    pub fn wait(&self) {
        while self.status() == Status::Downloading {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    pub fn take_failure(&self) -> Option<String> {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .failure
            .take()
    }

    pub fn remove(&self) -> std::io::Result<()> {
        let Some(binary) = self.binary else {
            return Ok(());
        };
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .verified = None;
        match std::fs::remove_file(binary.path_in(&self.dir)) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err),
            _ => Ok(()),
        }
    }

    pub fn load(&self) -> anyhow::Result<OpenH264API> {
        let binary = self
            .binary
            .ok_or_else(|| anyhow::anyhow!("recording isn't available on this system"))?;
        let path = binary.path_in(&self.dir);
        let bytes = std::fs::read(&path)
            .map_err(|_| anyhow::anyhow!("the video codec isn't downloaded"))?;
        if !binary.matches(&bytes) {
            self.state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .verified = Some(false);
            anyhow::bail!("the video codec file changed since it was downloaded");
        }
        let api = unsafe { OpenH264API::from_blob_path_unchecked(&path) }?;
        Ok(api)
    }
}

fn install(binary: Binary, dir: &Path, fetch: Fetch) -> anyhow::Result<PathBuf> {
    let compressed = fetch(&binary.url())?;
    let bytes = binary.unpack(&compressed)?;
    std::fs::create_dir_all(dir)?;
    let path = binary.path_in(dir);
    let part = path.with_extension("part");
    std::fs::write(&part, &bytes)?;
    std::fs::rename(&part, &path)?;
    Ok(path)
}

pub fn shared() -> &'static Codec {
    static CODEC: std::sync::LazyLock<Codec> = std::sync::LazyLock::new(|| {
        Codec::new(
            crate::storage::data_dir().join("codecs"),
            binary_for(std::env::consts::OS, std::env::consts::ARCH),
        )
    });
    &CODEC
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const FAKE: Binary = Binary {
        file: "libfake.so",
        sha256: "c1d1d2b1a5e1e7e5d0f8b5b1e3b4b6b0f7a0d5e9f7c8f2d2b5a6c3d1e0f9a8b7",
    };

    fn library() -> Vec<u8> {
        b"pretend this is a shared library".to_vec()
    }

    fn real_fake() -> Binary {
        Binary {
            file: FAKE.file,
            sha256: Box::leak(hex(&Sha256::digest(library())).into_boxed_str()),
        }
    }

    fn packed(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("citrine-codec-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn serve(bytes: Vec<u8>) -> Fetch {
        Box::new(move |_| Ok(bytes))
    }

    #[test]
    fn each_desktop_gets_its_own_build() {
        let pick = |os, arch| binary_for(os, arch).map(|b| b.url());
        let base = "https://ciscobinary.openh264.org/";
        assert_eq!(
            pick("macos", "aarch64"),
            Some(format!("{base}libopenh264-2.7.0-mac-arm64.dylib.bz2"))
        );
        assert_eq!(
            pick("macos", "x86_64"),
            Some(format!("{base}libopenh264-2.7.0-mac-x64.dylib.bz2"))
        );
        assert_eq!(
            pick("windows", "x86_64"),
            Some(format!("{base}openh264-2.7.0-win64.dll.bz2"))
        );
        assert_eq!(
            pick("linux", "x86_64"),
            Some(format!("{base}libopenh264-2.7.0-linux64.8.so.bz2"))
        );
        assert_eq!(
            pick("linux", "aarch64"),
            Some(format!("{base}libopenh264-2.7.0-linux-arm64.8.so.bz2"))
        );
        assert_eq!(pick("windows", "aarch64"), None);
        assert_eq!(pick("freebsd", "x86_64"), None);
    }

    #[test]
    fn every_build_has_a_full_checksum() {
        for binary in [MAC_ARM64, MAC_X64, WINDOWS_X64, LINUX_X64, LINUX_ARM64] {
            assert_eq!(binary.sha256.len(), 64, "{}", binary.file);
            assert!(binary.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn a_download_with_the_right_checksum_is_kept() {
        let binary = real_fake();
        assert_eq!(binary.unpack(&packed(&library())).unwrap(), library());
    }

    #[test]
    fn a_download_with_another_checksum_is_refused() {
        let err = FAKE.unpack(&packed(&library())).unwrap_err();
        assert!(err.to_string().contains("checksum"), "{err}");
        let err = FAKE.unpack(b"not bzip2 at all").unwrap_err();
        assert!(err.to_string().contains("damaged"), "{err}");
    }

    #[test]
    fn a_good_download_makes_the_codec_ready() {
        let dir = folder("good");
        let codec = Codec::new(dir.clone(), Some(real_fake()));
        assert_eq!(codec.status(), Status::Missing);
        assert_eq!(
            codec.answer(true, serve(packed(&library()))),
            Answer::Downloading
        );
        codec.wait();
        assert_eq!(codec.status(), Status::Ready);
        assert_eq!(codec.take_failure(), None);
        assert_eq!(std::fs::read(dir.join(FAKE.file)).unwrap(), library());
        assert_eq!(
            Codec::new(dir.clone(), Some(real_fake())).status(),
            Status::Ready
        );
        codec.remove().unwrap();
        assert_eq!(codec.status(), Status::Missing);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_bad_download_leaves_nothing_behind() {
        let dir = folder("bad");
        let codec = Codec::new(dir.clone(), Some(real_fake()));
        codec.download(serve(packed(b"something else")));
        codec.wait();
        assert_eq!(codec.status(), Status::Missing);
        assert!(codec.take_failure().unwrap().contains("checksum"));
        assert_eq!(codec.take_failure(), None);
        assert!(!dir.join(FAKE.file).exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failed_connection_is_reported() {
        let dir = folder("offline");
        let codec = Codec::new(dir.clone(), Some(real_fake()));
        codec.download(Box::new(|_| anyhow::bail!("no connection")));
        codec.wait();
        assert_eq!(codec.status(), Status::Missing);
        assert_eq!(codec.take_failure().as_deref(), Some("no connection"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn declining_downloads_nothing() {
        let dir = folder("declined");
        let codec = Codec::new(dir.clone(), Some(real_fake()));
        let answer = codec.answer(false, Box::new(|_| panic!("fetched after a decline")));
        assert_eq!(answer, Answer::Declined);
        assert_eq!(codec.status(), Status::Missing);
        assert_eq!(codec.take_failure(), None);
        assert!(!dir.exists());
        assert!(codec.load().is_err());
    }

    #[test]
    fn a_changed_file_is_not_loaded() {
        let dir = folder("changed");
        let codec = Codec::new(dir.clone(), Some(real_fake()));
        codec.download(serve(packed(&library())));
        codec.wait();
        assert_eq!(codec.status(), Status::Ready);
        std::fs::write(dir.join(FAKE.file), b"swapped").unwrap();
        let Err(err) = codec.load() else {
            panic!("a changed file was loaded");
        };
        assert!(err.to_string().contains("changed"), "{err}");
        assert_eq!(codec.status(), Status::Missing);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn other_systems_cannot_download() {
        let codec = Codec::new(folder("unsupported"), None);
        assert_eq!(codec.status(), Status::Unsupported);
        assert!(codec.load().is_err());
    }

    #[test]
    fn the_license_carries_the_notice() {
        assert!(LICENSE.contains(super::super::CODEC_NOTICE));
        assert!(LICENSE.contains("separately downloaded"));
    }
}

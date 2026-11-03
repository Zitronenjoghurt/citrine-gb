use serde::Deserialize;
use std::cmp::Ordering;

pub const SUMS: &str = "SHA256SUMS.txt";
pub const SIGNATURE: &str = "SHA256SUMS.txt.minisig";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version {
    numbers: [u64; 3],
    pre: bool,
}

impl Version {
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().trim_start_matches('v');
        let text = text.split_once('+').map_or(text, |(text, _)| text);
        let (core, pre) = match text.split_once('-') {
            Some((core, _)) => (core, true),
            None => (text, false),
        };
        let mut parts = core.split('.');
        let mut numbers = [0; 3];
        for number in &mut numbers {
            *number = parts.next()?.parse().ok()?;
        }
        if parts.next().is_some() {
            return None;
        }
        Some(Self { numbers, pre })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.numbers
            .cmp(&other.numbers)
            .then_with(|| other.pre.cmp(&self.pre))
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    #[serde(default)]
    pub size: u64,
    pub browser_download_url: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Release {
    pub tag_name: String,
    pub html_url: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

impl Release {
    pub fn parse(json: &[u8]) -> Option<Self> {
        serde_json::from_slice(json).ok()
    }

    pub fn version(&self) -> Option<Version> {
        Version::parse(&self.tag_name)
    }

    pub fn newer_than(&self, current: &str) -> bool {
        if self.draft || self.prerelease {
            return false;
        }
        match (self.version(), Version::parse(current)) {
            (Some(latest), Some(current)) => latest > current,
            _ => false,
        }
    }

    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|asset| asset.name == name)
    }

    pub fn download_for(&self, platform: Platform) -> Option<&Asset> {
        self.asset(&platform.asset_name(&self.tag_name))
    }

    pub fn signed_download_for(&self, platform: Platform) -> Option<&Asset> {
        self.asset(SUMS)?;
        self.asset(SIGNATURE)?;
        self.download_for(platform)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Android,
    LinuxX86_64,
    LinuxAarch64,
    Macos,
    WindowsX86_64,
}

impl Platform {
    pub fn current() -> Option<Self> {
        Self::of(std::env::consts::OS, std::env::consts::ARCH)
    }

    pub fn of(os: &str, arch: &str) -> Option<Self> {
        match (os, arch) {
            ("android", "aarch64") => Some(Self::Android),
            ("linux", "x86_64") => Some(Self::LinuxX86_64),
            ("linux", "aarch64") => Some(Self::LinuxAarch64),
            ("macos", "aarch64" | "x86_64") => Some(Self::Macos),
            ("windows", "x86_64") => Some(Self::WindowsX86_64),
            _ => None,
        }
    }

    pub fn asset_name(self, tag: &str) -> String {
        let suffix = match self {
            Self::Android => "android.apk",
            Self::LinuxX86_64 => "linux-x86_64.tar.gz",
            Self::LinuxAarch64 => "linux-aarch64.tar.gz",
            Self::Macos => "macos.dmg",
            Self::WindowsX86_64 => "windows-x86_64.zip",
        };
        format!("citrine-{tag}-{suffix}")
    }
}

pub fn sum_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        let file = file.trim_start().trim_start_matches('*');
        (file == name && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
}

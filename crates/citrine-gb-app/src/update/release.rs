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

#[cfg(test)]
mod tests {
    use super::super::tests::{SIGNED, Server};
    use super::{Platform, Version, sum_for};

    #[test]
    fn versions_compare_by_number_and_a_prerelease_comes_first() {
        let v = |text| Version::parse(text).unwrap();
        assert!(v("v0.10.0") > v("0.9.9"));
        assert!(v("0.10.1") > v("0.10.0"));
        assert!(v("1.0.0") > v("0.99.99"));
        assert!(v("0.11.0") > v("0.11.0-rc.1"));
        assert_eq!(v("v0.10.0"), v("0.10.0+build"));
        assert_eq!(Version::parse("0.10"), None);
        assert_eq!(Version::parse("0.10.0.1"), None);
        assert_eq!(Version::parse("latest"), None);
    }

    #[test]
    fn only_a_newer_published_release_counts() {
        let mut release = Server::release(Platform::Macos, &[]);
        assert!(release.newer_than("0.10.0"));
        assert!(!release.newer_than("0.11.0"));
        assert!(!release.newer_than("0.12.0"));
        release.prerelease = true;
        assert!(!release.newer_than("0.10.0"));
    }

    #[test]
    fn each_platform_picks_its_own_asset() {
        let names = [
            ("android", "aarch64", "citrine-v0.11.0-android.apk"),
            ("linux", "x86_64", "citrine-v0.11.0-linux-x86_64.tar.gz"),
            ("linux", "aarch64", "citrine-v0.11.0-linux-aarch64.tar.gz"),
            ("macos", "aarch64", "citrine-v0.11.0-macos.dmg"),
            ("macos", "x86_64", "citrine-v0.11.0-macos.dmg"),
            ("windows", "x86_64", "citrine-v0.11.0-windows-x86_64.zip"),
        ];
        let all: Vec<&str> = names.iter().map(|(_, _, name)| *name).collect();
        for (os, arch, name) in names {
            let platform = Platform::of(os, arch).unwrap();
            let release = Server::release(platform, &all);
            assert_eq!(release.download_for(platform).unwrap().name, name);
            assert!(release.signed_download_for(platform).is_none(), "{name}");
            let release = Server::release(platform, &[all.as_slice(), SIGNED].concat());
            assert_eq!(release.signed_download_for(platform).unwrap().name, name);
        }
        assert_eq!(Platform::of("windows", "aarch64"), None);
        assert_eq!(Platform::of("freebsd", "x86_64"), None);
    }

    #[test]
    fn a_checksum_line_matches_only_its_own_file() {
        let sums = format!(
            "{}  citrine-v0.11.0-linux-x86_64.tar.gz\n{} *citrine-v0.11.0-windows-x86_64.zip\nnot a line\n",
            "a".repeat(64),
            "B".repeat(64)
        );
        assert_eq!(
            sum_for(&sums, "citrine-v0.11.0-linux-x86_64.tar.gz"),
            Some("a".repeat(64))
        );
        assert_eq!(
            sum_for(&sums, "citrine-v0.11.0-windows-x86_64.zip"),
            Some("b".repeat(64))
        );
        assert_eq!(sum_for(&sums, "citrine-v0.11.0-linux"), None);
        assert_eq!(sum_for("abc  citrine.apk\n", "citrine.apk"), None);
    }
}

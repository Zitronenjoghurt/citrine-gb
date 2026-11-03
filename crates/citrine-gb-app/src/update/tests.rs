use super::install::{binary_from_tar_gz, binary_from_zip, bundle_of, replace_file};
use super::release::{Platform, Release, Version, sum_for};
use super::worker::{Backend, PUBLIC_KEY, verify};
use super::{Phase, Tap, Updater};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Cursor, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

const TAG: &str = "v0.11.0";

fn keys() -> minisign::KeyPair {
    minisign::KeyPair::generate_unencrypted_keypair().unwrap()
}

fn sign(keys: &minisign::KeyPair, data: &[u8]) -> Vec<u8> {
    minisign::sign(Some(&keys.pk), &keys.sk, Cursor::new(data), Some(TAG), None)
        .unwrap()
        .to_string()
        .into_bytes()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn tar_gz(dir: &str, name: &str, binary: &[u8]) -> Vec<u8> {
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

fn zip_of(dir: &str, name: &str, binary: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file(format!("{dir}/CHANGELOG.md"), options)
        .unwrap();
    zip.write_all(b"changes").unwrap();
    zip.start_file(format!("{dir}/{name}"), options).unwrap();
    zip.write_all(binary).unwrap();
    zip.finish().unwrap().into_inner()
}

struct Server {
    files: HashMap<String, Vec<u8>>,
}

const BASE: &str = "https://example.test/";

impl Server {
    fn release(platform: Platform, names: &[&str]) -> Release {
        Release::parse(Self::release_json(platform, names).as_bytes()).unwrap()
    }

    fn release_json(platform: Platform, names: &[&str]) -> String {
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

    fn new(
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

    fn updater(self, key: Option<String>, installed: Arc<Mutex<Vec<Vec<u8>>>>) -> Updater {
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

const SIGNED: &[&str] = &["SHA256SUMS.txt", "SHA256SUMS.txt.minisig"];

fn run(updater: &mut Updater) -> Phase {
    updater.tick(true);
    updater.wait();
    if matches!(updater.phase(), Phase::Available(_)) {
        updater.tap();
        updater.wait();
    }
    updater.phase()
}

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

#[test]
fn a_signature_verifies_only_with_its_key_and_its_data() {
    let keys = keys();
    let key = keys.pk.to_base64();
    let signature = sign(&keys, b"sums");
    assert!(verify(&key, b"sums", &signature));
    assert!(verify(
        &keys.pk.to_box().unwrap().to_string(),
        b"sums",
        &signature
    ));
    assert!(!verify(&key, b"sumz", &signature));
    assert!(!verify(&self::keys().pk.to_base64(), b"sums", &signature));
    assert!(!verify(&key, b"sums", b"untrusted comment: nothing\n"));
    assert!(!verify("not a key", b"sums", &signature));
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
    assert_eq!(updater.tap(), Tap::Restart);
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
    assert!(matches!(updater.tap(), Tap::Open(_)));
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
    assert_eq!(updater.tap(), Tap::Nothing);
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
    updater.tick(true);
    updater.wait();
    let Phase::Available(offer) = updater.phase() else {
        panic!("{:?}", updater.phase());
    };
    assert!(!offer.installable);
    assert_eq!(offer.version, "0.11.0");
    assert_eq!(updater.tap(), Tap::Open(offer.page));
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
    updater.tick(true);
    updater.wait();
    let Phase::Available(offer) = updater.phase() else {
        panic!("{:?}", updater.phase());
    };
    assert!(!offer.installable);
    assert!(matches!(updater.tap(), Tap::Open(_)));
}

#[test]
fn the_switch_off_means_no_check() {
    let server = Server::new(SIGNED, b"x", b"", None);
    let mut updater = server.updater(None, Arc::default());
    updater.tick(false);
    updater.wait();
    assert_eq!(updater.phase(), Phase::Idle);
}

#[test]
fn the_app_finds_its_binary_in_each_archive() {
    let tar = tar_gz("citrine-v0.11.0-linux-x86_64", "citrine", b"elf");
    assert_eq!(binary_from_tar_gz(&tar, "citrine").unwrap(), b"elf");
    let nested = tar_gz("a/b", "citrine", b"elf");
    assert!(binary_from_tar_gz(&nested, "citrine").is_err());
    let zip = zip_of("citrine-v0.11.0-windows-x86_64", "citrine.exe", b"pe");
    assert_eq!(binary_from_zip(&zip, "citrine.exe").unwrap(), b"pe");
    assert!(binary_from_zip(&zip, "citrine").is_err());
    assert!(binary_from_tar_gz(b"junk", "citrine").is_err());
}

#[test]
fn the_new_binary_takes_the_old_ones_place() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("citrine");
    std::fs::write(&target, b"old").unwrap();
    replace_file(&target, b"new").unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"new");
    assert!(!dir.path().join(".citrine-update").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&target).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111);
    }
}

#[test]
fn a_mac_update_needs_the_app_bundle() {
    let bundle = bundle_of(Path::new(
        "/Applications/Citrine.app/Contents/MacOS/citrine",
    ))
    .unwrap();
    assert_eq!(bundle, Path::new("/Applications/Citrine.app"));
    assert!(bundle_of(Path::new("/usr/local/bin/citrine")).is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn a_mac_update_swaps_the_whole_bundle() {
    use std::process::Command;
    let dir = tempfile::tempdir().unwrap();
    let bundle = |root: &Path, marker: &str| {
        let app = root.join("Citrine.app");
        let macos = app.join("Contents/MacOS");
        std::fs::create_dir_all(&macos).unwrap();
        std::fs::copy("/usr/bin/true", macos.join("citrine")).unwrap();
        std::fs::create_dir_all(app.join("Contents/Resources")).unwrap();
        std::fs::write(
            app.join("Contents/Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>citrine</string>
<key>CFBundleIdentifier</key><string>io.github.zitronenjoghurt.citrine.test</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>"#,
        )
        .unwrap();
        std::fs::write(app.join("Contents/Resources/marker.txt"), marker).unwrap();
        let signed = Command::new("codesign")
            .args(["--force", "--deep", "--sign", "-"])
            .arg(&app)
            .status()
            .unwrap();
        assert!(signed.success());
        let check = Command::new("codesign")
            .args(["--verify", "--deep", "--strict", "--verbose=2"])
            .arg(&app)
            .output()
            .unwrap();
        assert!(
            check.status.success(),
            "{}",
            String::from_utf8_lossy(&check.stderr)
        );
        app
    };
    let source = dir.path().join("source");
    bundle(&source, "new");
    let image = dir.path().join("update.dmg");
    let made = Command::new("hdiutil")
        .args([
            "create",
            "-quiet",
            "-format",
            "UDZO",
            "-volname",
            "Citrine",
            "-srcfolder",
        ])
        .arg(&source)
        .arg(&image)
        .status()
        .unwrap();
    assert!(made.success());
    let apps = dir.path().join("Applications");
    let installed = bundle(&apps, "old");
    super::install::replace_bundle(&std::fs::read(&image).unwrap(), &installed).unwrap();
    assert_eq!(
        std::fs::read_to_string(installed.join("Contents/Resources/marker.txt")).unwrap(),
        "new"
    );
    let left: Vec<_> = std::fs::read_dir(&apps)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left, ["Citrine.app"]);
}

#[test]
fn the_built_in_key_decodes() {
    let key = PUBLIC_KEY.unwrap();
    assert!(minisign_verify::PublicKey::from_base64(key).is_ok());
}

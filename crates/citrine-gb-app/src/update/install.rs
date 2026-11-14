use super::release::Asset;
use super::worker::Install;
use anyhow::{Context, bail};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

static EXE: OnceLock<Option<PathBuf>> = OnceLock::new();

pub fn remember_exe() {
    EXE.get_or_init(|| std::env::current_exe().ok());
}

fn exe() -> anyhow::Result<PathBuf> {
    remember_exe();
    EXE.get()
        .cloned()
        .flatten()
        .context("Citrine couldn't find its own program file.")
}

pub fn live() -> Install {
    remember_exe();
    Arc::new(|bytes: &[u8], asset: &Asset| install(bytes, asset))
}

#[cfg(any(target_os = "linux", windows))]
fn install(bytes: &[u8], asset: &Asset) -> anyhow::Result<()> {
    let binary = if asset.name.ends_with(".zip") {
        binary_from_zip(bytes, "citrine.exe")?
    } else {
        binary_from_tar_gz(bytes, "citrine")?
    };
    replace_file(&exe()?, &binary)
}

#[cfg(target_os = "macos")]
fn install(bytes: &[u8], _asset: &Asset) -> anyhow::Result<()> {
    replace_bundle(bytes, &bundle_of(&exe()?)?)
}

#[cfg(target_os = "android")]
fn install(bytes: &[u8], _asset: &Asset) -> anyhow::Result<()> {
    let path = apk_path()?;
    std::fs::write(&path, bytes).context("Citrine couldn't save the update.")?;
    hand_to_installer(&path)
}

#[cfg(not(any(
    target_os = "linux",
    windows,
    target_os = "macos",
    target_os = "android"
)))]
fn install(_bytes: &[u8], _asset: &Asset) -> anyhow::Result<()> {
    bail!("Not supported on this system")
}

#[cfg(any(target_os = "linux", windows, test))]
pub fn binary_from_tar_gz(archive: &[u8], name: &str) -> anyhow::Result<Vec<u8>> {
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    for entry in tar
        .entries()
        .context("The download is not a valid archive.")?
    {
        let mut entry = entry.context("The download is not a valid archive.")?;
        let path = entry.path()?.into_owned();
        if path.file_name().is_some_and(|file| file == name) && path.components().count() == 2 {
            let mut binary = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut binary)?;
            return Ok(binary);
        }
    }
    bail!("The download is incomplete")
}

#[cfg(any(target_os = "linux", windows, test))]
pub fn binary_from_zip(archive: &[u8], name: &str) -> anyhow::Result<Vec<u8>> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive))
        .context("The download is not a valid archive.")?;
    for index in 0..zip.len() {
        let mut file = zip.by_index(index)?;
        let path = file.enclosed_name().unwrap_or_default();
        if path.file_name().is_some_and(|file| file == name) && path.components().count() == 2 {
            let mut binary = Vec::new();
            std::io::Read::read_to_end(&mut file, &mut binary)?;
            return Ok(binary);
        }
    }
    bail!("The download is incomplete")
}

#[cfg(any(target_os = "linux", windows, target_os = "macos", test))]
fn no_write(dir: &Path, err: std::io::Error) -> anyhow::Error {
    log::warn!("update can't write {}: {err}", dir.display());
    if err.kind() == std::io::ErrorKind::PermissionDenied {
        anyhow::anyhow!("Its folder is read-only")
    } else {
        anyhow::anyhow!("Couldn't replace the app")
    }
}

#[cfg(any(target_os = "linux", windows, test))]
pub fn replace_file(target: &Path, binary: &[u8]) -> anyhow::Result<()> {
    let dir = target
        .parent()
        .context("Citrine couldn't find its own folder.")?;
    let staged = dir.join(".citrine-update");
    std::fs::write(&staged, binary).map_err(|err| no_write(dir, err))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
    }
    let replaced = if Some(target) == EXE.get().cloned().flatten().as_deref() {
        self_replace::self_replace(&staged)
    } else {
        std::fs::rename(&staged, target)
    };
    let _ = std::fs::remove_file(&staged);
    replaced.map_err(|err| no_write(dir, err))
}

#[cfg(any(target_os = "macos", test))]
pub fn bundle_of(exe: &Path) -> anyhow::Result<PathBuf> {
    let bundle = exe
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .filter(|app| app.extension().is_some_and(|ext| ext == "app"))
        .context("Citrine isn't running from its app bundle, so it can't replace itself.")?;
    Ok(bundle.to_path_buf())
}

#[cfg(target_os = "macos")]
pub fn replace_bundle(dmg: &[u8], bundle: &Path) -> anyhow::Result<()> {
    let dir = bundle
        .parent()
        .context("Citrine couldn't find its own folder.")?;
    let work = std::env::temp_dir().join(format!("citrine-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    let image = work.join("update.dmg");
    let mount = work.join("mount");
    std::fs::create_dir_all(&mount)?;
    std::fs::write(&image, dmg)?;
    let result = swap_bundle(&image, &mount, bundle, dir);
    let _ = std::fs::remove_dir_all(&work);
    result
}

#[cfg(target_os = "macos")]
fn swap_bundle(image: &Path, mount: &Path, bundle: &Path, dir: &Path) -> anyhow::Result<()> {
    use std::process::Command;
    let attached = Command::new("hdiutil")
        .args([
            "attach",
            "-nobrowse",
            "-readonly",
            "-noautoopen",
            "-mountpoint",
        ])
        .arg(mount)
        .arg(image)
        .output()?;
    if !attached.status.success() {
        bail!("The disk image didn't open");
    }
    let staged = dir.join(".Citrine-update.app");
    let old = dir.join(".Citrine-old.app");
    let _ = std::fs::remove_dir_all(&staged);
    let _ = std::fs::remove_dir_all(&old);
    let copied = Command::new("ditto")
        .arg(mount.join("Citrine.app"))
        .arg(&staged)
        .status();
    let _ = Command::new("hdiutil")
        .args(["detach", "-quiet"])
        .arg(mount)
        .status();
    if !copied.is_ok_and(|status| status.success()) {
        let _ = std::fs::remove_dir_all(&staged);
        bail!("Its folder is read-only");
    }
    let verified = Command::new("codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(&staged)
        .status();
    if !verified.is_ok_and(|status| status.success()) {
        let _ = std::fs::remove_dir_all(&staged);
        bail!("The app signature is broken");
    }
    std::fs::rename(bundle, &old).map_err(|err| no_write(dir, err))?;
    if let Err(err) = std::fs::rename(&staged, bundle) {
        let _ = std::fs::rename(&old, bundle);
        return Err(no_write(dir, err));
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

pub fn relaunch() {
    let Ok(exe) = exe() else {
        return;
    };
    #[cfg(target_os = "macos")]
    if let Ok(bundle) = bundle_of(&exe) {
        let _ = std::process::Command::new("open")
            .arg("-n")
            .arg(bundle)
            .spawn();
        return;
    }
    let _ = std::process::Command::new(exe)
        .args(std::env::args_os().skip(1))
        .spawn();
}

#[cfg(target_os = "android")]
fn apk_path() -> anyhow::Result<PathBuf> {
    let dir = crate::storage::cache_dir().join(crate::storage::UPDATES);
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("citrine-update.apk"))
}

#[cfg(target_os = "android")]
fn hand_to_installer(path: &Path) -> anyhow::Result<()> {
    use jni::objects::JValue;
    let path = path.to_string_lossy().into_owned();
    let answer = crate::platform::android::java(|env, activity| {
        let path = env.new_string(&path)?;
        env.call_method(
            activity,
            "installUpdate",
            "(Ljava/lang/String;)I",
            &[JValue::Object(&path)],
        )?
        .i()
    });
    match answer {
        Some(0) => Ok(()),
        Some(1) => bail!("Signed with another key"),
        _ => bail!("The installer didn't open"),
    }
}

#[cfg(target_os = "android")]
pub fn reopen() {
    if let Ok(path) = apk_path() {
        let _ = hand_to_installer(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::tar_gz;
    use super::{binary_from_tar_gz, binary_from_zip, bundle_of, replace_file};
    use std::io::Cursor;
    use std::io::Write;
    use std::path::Path;

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
        super::replace_bundle(&std::fs::read(&image).unwrap(), &installed).unwrap();
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
}

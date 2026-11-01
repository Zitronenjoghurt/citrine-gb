use std::path::Path;

#[cfg(target_os = "macos")]
pub const LABEL: &str = "Show in Finder";
#[cfg(target_os = "windows")]
pub const LABEL: &str = "Show in Explorer";
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub const LABEL: &str = "Show in Files";

#[cfg(target_os = "macos")]
pub fn reveal(path: &Path) -> std::io::Result<()> {
    std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn()
        .map(drop)
}

#[cfg(target_os = "windows")]
pub fn reveal(path: &Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("explorer.exe")
        .raw_arg(format!("/select,\"{}\"", path.display()))
        .spawn()
        .map(drop)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn reveal(path: &Path) -> std::io::Result<()> {
    let path = path.to_path_buf();
    std::thread::spawn(move || {
        let shown = std::process::Command::new("dbus-send")
            .args([
                "--session",
                "--print-reply",
                "--reply-timeout=3000",
                "--dest=org.freedesktop.FileManager1",
                "--type=method_call",
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1.ShowItems",
            ])
            .arg(format!("array:string:{}", file_uri(&path)))
            .arg("string:")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if !shown {
            let folder = path.parent().unwrap_or(&path);
            let _ = std::process::Command::new("xdg-open").arg(folder).status();
        }
    });
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                uri.push(byte as char)
            }
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_names_finder() {
        assert_eq!(LABEL, "Show in Finder");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_names_explorer() {
        assert_eq!(LABEL, "Show in Explorer");
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    #[test]
    fn linux_names_files() {
        assert_eq!(LABEL, "Show in Files");
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    #[test]
    fn file_uris_escape_spaces_and_unicode() {
        let uri = file_uri(Path::new("/home/c/media/POKéMON 2026-01-02 03-04-05.png"));
        assert_eq!(
            uri,
            "file:///home/c/media/POK%C3%A9MON%202026-01-02%2003-04-05.png"
        );
    }
}

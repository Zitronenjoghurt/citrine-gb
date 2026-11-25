pub trait Secrets: Send {
    fn get(&self, username: &str) -> Option<String>;
    fn set(&self, username: &str, token: &str) -> bool;
    fn remove(&self, username: &str);
}

pub struct InFile;

impl Secrets for InFile {
    fn get(&self, _username: &str) -> Option<String> {
        None
    }

    fn set(&self, _username: &str, _token: &str) -> bool {
        false
    }

    fn remove(&self, _username: &str) {}
}

#[cfg(any(windows, target_os = "linux"))]
pub struct System {
    warned: std::sync::atomic::AtomicBool,
}

#[cfg(any(windows, target_os = "linux"))]
const SERVICE: &str = "Citrine RetroAchievements";

#[cfg(any(windows, target_os = "linux"))]
impl System {
    fn entry(&self, username: &str) -> Option<keyring::Entry> {
        keyring::Entry::new(SERVICE, username)
            .map_err(|error| self.warn(&error))
            .ok()
    }

    fn warn(&self, error: &keyring::Error) {
        if !self.warned.swap(true, std::sync::atomic::Ordering::Relaxed) {
            log::warn!(
                "system keyring unavailable, the RetroAchievements token stays in the settings file: {error}"
            );
        }
    }
}

#[cfg(any(windows, target_os = "linux"))]
impl Secrets for System {
    fn get(&self, username: &str) -> Option<String> {
        match self.entry(username)?.get_password() {
            Ok(token) => Some(token),
            Err(keyring::Error::NoEntry) => None,
            Err(error) => {
                self.warn(&error);
                None
            }
        }
    }

    fn set(&self, username: &str, token: &str) -> bool {
        let Some(entry) = self.entry(username) else {
            return false;
        };
        match entry.set_password(token) {
            Ok(()) => true,
            Err(error) => {
                self.warn(&error);
                false
            }
        }
    }

    fn remove(&self, username: &str) {
        if let Some(entry) = self.entry(username) {
            let _ = entry.delete_credential();
        }
    }
}

#[cfg(any(target_os = "android", test))]
pub const TOKEN_DIR: crate::storage::kinds::Folder =
    crate::storage::kinds::Folder::AchievementsToken;

#[cfg(any(target_os = "android", test))]
const TOKEN_FILE: &str = "tokens.json";

#[cfg(any(target_os = "android", test))]
pub struct TokenFile {
    backend: crate::storage::Folders,
}

#[cfg(any(target_os = "android", test))]
impl TokenFile {
    pub fn new(backend: crate::storage::Folders) -> Self {
        Self { backend }
    }

    fn tokens(&self) -> std::collections::BTreeMap<String, String> {
        self.backend
            .read(TOKEN_DIR, TOKEN_FILE)
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn keep(&self, tokens: &std::collections::BTreeMap<String, String>) -> bool {
        let written = if tokens.is_empty() {
            self.backend.delete(TOKEN_DIR, TOKEN_FILE)
        } else {
            serde_json::to_vec(tokens)
                .map_err(std::io::Error::other)
                .and_then(|bytes| self.backend.write(TOKEN_DIR, TOKEN_FILE, &bytes))
        };
        written
            .map_err(|error| log::warn!("could not keep the RetroAchievements token: {error}"))
            .is_ok()
    }
}

#[cfg(any(target_os = "android", test))]
impl Secrets for TokenFile {
    fn get(&self, username: &str) -> Option<String> {
        self.tokens().remove(username)
    }

    fn set(&self, username: &str, token: &str) -> bool {
        let mut tokens = self.tokens();
        tokens.insert(username.to_owned(), token.to_owned());
        self.keep(&tokens)
    }

    fn remove(&self, username: &str) {
        let mut tokens = self.tokens();
        if tokens.remove(username).is_some() {
            self.keep(&tokens);
        }
    }
}

pub fn platform() -> Box<dyn Secrets> {
    #[cfg(any(windows, target_os = "linux"))]
    {
        Box::new(System {
            warned: std::sync::atomic::AtomicBool::new(false),
        })
    }
    #[cfg(target_os = "android")]
    {
        Box::new(TokenFile::new(crate::storage::Folders::default()))
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "android")))]
    {
        Box::new(InFile)
    }
}

#[cfg(test)]
#[expect(
    clippy::disallowed_types,
    reason = "the fake keyring and the test that reads what it kept share one map"
)]
#[derive(Clone, Default)]
pub struct Memory {
    pub tokens: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>,
    pub broken: bool,
}

#[cfg(test)]
impl Secrets for Memory {
    fn get(&self, username: &str) -> Option<String> {
        self.tokens.lock().unwrap().get(username).cloned()
    }

    fn set(&self, username: &str, token: &str) -> bool {
        if self.broken {
            return false;
        }
        self.tokens
            .lock()
            .unwrap()
            .insert(username.to_owned(), token.to_owned());
        true
    }

    fn remove(&self, username: &str) {
        self.tokens.lock().unwrap().remove(username);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn android_file(path: &str) -> String {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../android/app/src/main");
        std::fs::read_to_string(root.join(path)).expect(path)
    }

    #[test]
    fn the_token_file_keeps_tokens_by_account_and_goes_when_empty() {
        let root = crate::storage::scratch_dir()
            .join(format!("citrine-token-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let file = TokenFile::new(crate::storage::Folders::at(root.clone()));
        assert_eq!(file.get("ClaraPlays"), None);
        assert!(file.set("ClaraPlays", "ApiToken"));
        assert!(file.set("Speedy", "Other"));
        assert_eq!(file.get("ClaraPlays").as_deref(), Some("ApiToken"));
        assert!(root.join(TOKEN_DIR.name()).join(TOKEN_FILE).is_file());
        file.remove("ClaraPlays");
        assert_eq!(file.get("ClaraPlays"), None);
        assert_eq!(file.get("Speedy").as_deref(), Some("Other"));
        file.remove("Speedy");
        assert!(!root.join(TOKEN_DIR.name()).join(TOKEN_FILE).exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn android_backups_leave_out_the_token_folder_and_the_lan_keys() {
        let root =
            crate::storage::scratch_dir().join(format!("citrine-lan-keys-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let lan_keys = "settings/lan-keys.dat";
        crate::storage::SettingsStore::at(root.clone())
            .store_lan_keyring(b"keys")
            .unwrap();
        assert!(root.join(lan_keys).is_file());
        let _ = std::fs::remove_dir_all(&root);
        let full = android_file("res/xml/backup_rules.xml");
        let extraction = android_file("res/xml/data_extraction_rules.xml");
        for path in [TOKEN_DIR.name(), lan_keys] {
            let exclude = format!(r#"<exclude domain="file" path="{path}" />"#);
            assert_eq!(full.matches(&exclude).count(), 1, "{full}");
            for section in ["cloud-backup", "device-transfer"] {
                let start = extraction.find(&format!("<{section}>")).expect(section);
                let end = extraction.find(&format!("</{section}>")).expect(section);
                assert!(extraction[start..end].contains(&exclude), "{extraction}");
            }
        }
        let manifest = android_file("AndroidManifest.xml");
        assert!(manifest.contains(r#"android:fullBackupContent="@xml/backup_rules""#));
        assert!(manifest.contains(r#"android:dataExtractionRules="@xml/data_extraction_rules""#));
    }
}

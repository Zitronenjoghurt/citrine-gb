#[cfg(not(target_arch = "wasm32"))]
pub use native::Backend;
#[cfg(target_arch = "wasm32")]
pub use web::Backend;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use directories::ProjectDirs;
    use std::path::PathBuf;

    const FALLBACK_DIR: &str = "citrine-data";

    pub struct Backend {
        root: PathBuf,
    }

    impl Default for Backend {
        fn default() -> Self {
            let root = match ProjectDirs::from("", "", "citrine") {
                Some(dirs) => dirs.data_dir().join("saves"),
                None => PathBuf::from(FALLBACK_DIR).join("saves"),
            };
            Self { root }
        }
    }

    impl Backend {
        #[cfg(test)]
        pub fn at(root: PathBuf) -> Self {
            Self { root }
        }

        fn path(&self, namespace: &str, name: &str) -> PathBuf {
            self.root.join(namespace).join(name)
        }

        pub fn read(&self, namespace: &str, name: &str) -> Option<Vec<u8>> {
            std::fs::read(self.path(namespace, name)).ok()
        }

        pub fn write(&self, namespace: &str, name: &str, data: &[u8]) -> std::io::Result<()> {
            let path = self.path(namespace, name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let temp = path.with_extension("tmp");
            std::fs::write(&temp, data)?;
            std::fs::rename(&temp, &path)
        }

        pub fn delete(&self, namespace: &str, name: &str) -> std::io::Result<()> {
            match std::fs::remove_file(self.path(namespace, name)) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                other => other,
            }
        }

        pub fn exists(&self, namespace: &str, name: &str) -> bool {
            self.path(namespace, name).is_file()
        }

        pub fn list(&self, namespace: &str) -> Vec<String> {
            let Ok(entries) = std::fs::read_dir(self.root.join(namespace)) else {
                return Vec::new();
            };
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        }

        pub fn location(&self) -> String {
            self.root.display().to_string()
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use base64::Engine;

    #[derive(Default)]
    pub struct Backend;

    fn storage() -> Option<web_sys::Storage> {
        web_sys::window()?.local_storage().ok()?
    }

    fn key(namespace: &str, name: &str) -> String {
        format!("citrine/{namespace}/{name}")
    }

    impl Backend {
        pub fn read(&self, namespace: &str, name: &str) -> Option<Vec<u8>> {
            let raw = storage()?.get_item(&key(namespace, name)).ok()??;
            base64::engine::general_purpose::STANDARD.decode(raw).ok()
        }

        pub fn write(&self, namespace: &str, name: &str, data: &[u8]) -> std::io::Result<()> {
            let encoded = base64::engine::general_purpose::STANDARD.encode(data);
            storage()
                .ok_or_else(|| std::io::Error::other("local storage unavailable"))?
                .set_item(&key(namespace, name), &encoded)
                .map_err(|_| std::io::Error::other("local storage write failed (quota?)"))
        }

        pub fn delete(&self, namespace: &str, name: &str) -> std::io::Result<()> {
            if let Some(storage) = storage() {
                let _ = storage.remove_item(&key(namespace, name));
            }
            Ok(())
        }

        pub fn exists(&self, namespace: &str, name: &str) -> bool {
            self.read(namespace, name).is_some()
        }

        pub fn list(&self, namespace: &str) -> Vec<String> {
            let Some(storage) = storage() else {
                return Vec::new();
            };
            let prefix = format!("citrine/{namespace}/");
            let count = storage.length().unwrap_or(0);
            (0..count)
                .filter_map(|i| storage.key(i).ok().flatten())
                .filter_map(|k| k.strip_prefix(&prefix).map(str::to_owned))
                .collect()
        }

        pub fn location(&self) -> String {
            "browser local storage".to_string()
        }
    }
}

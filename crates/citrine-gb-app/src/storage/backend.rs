#[cfg(not(target_arch = "wasm32"))]
pub use native::{Backend, data_dir};
#[cfg(target_arch = "wasm32")]
pub use web::{Backend, keep_crash_save, open_saves};

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use directories::ProjectDirs;
    use std::path::PathBuf;

    const FALLBACK_DIR: &str = "citrine-data";

    #[derive(Clone)]
    pub struct Backend {
        root: PathBuf,
    }

    pub fn data_dir() -> PathBuf {
        #[cfg(target_os = "android")]
        if let Some(dir) = crate::platform::android::data_dir() {
            return dir;
        }
        match ProjectDirs::from("", "", "citrine") {
            Some(dirs) => dirs.data_dir().to_path_buf(),
            None => PathBuf::from(FALLBACK_DIR),
        }
    }

    impl Default for Backend {
        fn default() -> Self {
            Self {
                root: data_dir().join("saves"),
            }
        }
    }

    impl Backend {
        pub fn settings() -> Self {
            Self { root: data_dir() }
        }

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

        pub fn size(&self, namespace: &str, name: &str) -> u64 {
            std::fs::metadata(self.path(namespace, name)).map_or(0, |m| m.len())
        }

        pub fn clear(&self, namespace: &str) -> std::io::Result<()> {
            match std::fs::remove_dir_all(self.root.join(namespace)) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                other => other,
            }
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

        pub fn namespaces(&self) -> Vec<String> {
            let Ok(entries) = std::fs::read_dir(&self.root) else {
                return Vec::new();
            };
            entries
                .flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        }

        pub fn location(&self) -> String {
            self.root.display().to_string()
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod web;

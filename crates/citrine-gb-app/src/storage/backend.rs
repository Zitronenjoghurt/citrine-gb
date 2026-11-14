#[cfg(all(test, not(target_arch = "wasm32")))]
pub use native::TestDirs;
#[cfg(not(target_arch = "wasm32"))]
pub use native::{Backend, cache_dir, data_dir};
#[cfg(target_arch = "wasm32")]
pub use web::{Backend, keep_crash_save, open_saves};

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::path::PathBuf;

    #[cfg(not(test))]
    const FALLBACK_DIR: &str = "citrine-data";
    #[cfg(not(test))]
    const FALLBACK_CACHE_DIR: &str = "citrine-cache";

    #[derive(Clone)]
    pub struct Backend {
        root: PathBuf,
    }

    #[cfg(test)]
    thread_local! {
        static TEST_DIRS: std::cell::RefCell<Option<(PathBuf, PathBuf)>> =
            const { std::cell::RefCell::new(None) };
    }

    #[cfg(test)]
    pub struct TestDirs {
        pub data: PathBuf,
        pub cache: PathBuf,
    }

    #[cfg(test)]
    impl TestDirs {
        pub fn new(name: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("citrine-dirs-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            let dirs = Self {
                data: root.join("data"),
                cache: root.join("cache"),
            };
            TEST_DIRS.set(Some((dirs.data.clone(), dirs.cache.clone())));
            dirs
        }
    }

    #[cfg(test)]
    impl Drop for TestDirs {
        fn drop(&mut self) {
            TEST_DIRS.set(None);
            if let Some(root) = self.data.parent() {
                let _ = std::fs::remove_dir_all(root);
            }
        }
    }

    #[cfg(test)]
    fn test_dir(cache: bool) -> PathBuf {
        TEST_DIRS
            .with_borrow(|dirs| {
                dirs.as_ref()
                    .map(|(data, cached)| if cache { cached } else { data }.clone())
            })
            .unwrap_or_else(|| {
                let suffix = if cache { "-cache" } else { "" };
                std::env::temp_dir().join(format!("citrine-test-{}{suffix}", std::process::id()))
            })
    }

    #[cfg(test)]
    pub fn data_dir() -> PathBuf {
        test_dir(false)
    }

    #[cfg(test)]
    pub fn cache_dir() -> PathBuf {
        test_dir(true)
    }

    #[cfg(not(test))]
    pub fn data_dir() -> PathBuf {
        #[cfg(target_os = "android")]
        if let Some(dir) = crate::platform::android::data_dir() {
            return dir;
        }
        match directories::ProjectDirs::from("", "", "citrine") {
            Some(dirs) => dirs.data_dir().to_path_buf(),
            None => PathBuf::from(FALLBACK_DIR),
        }
    }

    #[cfg(not(test))]
    pub fn cache_dir() -> PathBuf {
        #[cfg(target_os = "android")]
        if let Some(dir) = crate::platform::android::cache_dir() {
            return dir;
        }
        match directories::ProjectDirs::from("", "", "citrine") {
            Some(dirs) => dirs.cache_dir().to_path_buf(),
            None => PathBuf::from(FALLBACK_CACHE_DIR),
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

        pub fn cache() -> Self {
            Self { root: cache_dir() }
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

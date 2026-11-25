use super::Backend;
use super::kinds::{Folder, Root};

#[derive(Clone)]
pub struct Folders {
    data: Backend,
    cache: Backend,
}

impl Default for Folders {
    fn default() -> Self {
        Self {
            data: Backend::settings(),
            cache: Backend::cache(),
        }
    }
}

impl Folders {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn at(root: std::path::PathBuf) -> Self {
        Self::in_dirs(root.clone(), root)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn in_dirs(data: std::path::PathBuf, cache: std::path::PathBuf) -> Self {
        Self {
            data: Backend::at(data),
            cache: Backend::at(cache),
        }
    }

    fn backend(&self, folder: Folder) -> &Backend {
        match folder.root() {
            Root::Data => &self.data,
            Root::Cache => &self.cache,
        }
    }

    pub fn read(&self, folder: Folder, name: &str) -> Option<Vec<u8>> {
        self.backend(folder).read(folder.name(), name)
    }

    pub fn write(&self, folder: Folder, name: &str, data: &[u8]) -> std::io::Result<()> {
        self.backend(folder).write(folder.name(), name, data)
    }

    pub fn delete(&self, folder: Folder, name: &str) -> std::io::Result<()> {
        self.backend(folder).delete(folder.name(), name)
    }

    pub fn exists(&self, folder: Folder, name: &str) -> bool {
        self.backend(folder).exists(folder.name(), name)
    }

    #[cfg(target_arch = "wasm32")]
    pub fn size(&self, folder: Folder, name: &str) -> u64 {
        self.backend(folder).size(folder.name(), name)
    }

    pub fn clear(&self, folder: Folder) -> std::io::Result<()> {
        self.backend(folder).clear(folder.name())
    }

    pub fn list(&self, folder: Folder) -> Vec<String> {
        self.backend(folder).list(folder.name())
    }

    pub fn location(&self) -> String {
        self.data.location()
    }

    pub(super) fn clear_cache(&self) -> std::io::Result<()> {
        self.cache.clear_all()
    }
}

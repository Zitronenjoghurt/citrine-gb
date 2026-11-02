use rfd::AsyncFileDialog;
use std::sync::mpsc::Sender;

pub struct PickedFile {
    pub name: String,
    pub data: Vec<u8>,
    #[cfg(not(target_arch = "wasm32"))]
    pub path: Option<std::path::PathBuf>,
}

impl PickedFile {
    pub fn from_drop(file: egui::DroppedFile) -> Option<Self> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = file.path {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let data = std::fs::read(&path).ok()?;
            return Some(Self {
                name,
                data,
                path: Some(path),
            });
        }
        let data = file.bytes?.to_vec();
        Some(Self {
            name: file.name,
            data,
            #[cfg(not(target_arch = "wasm32"))]
            path: None,
        })
    }
}

#[derive(Default)]
pub struct FileLoader {
    dialog: AsyncFileDialog,
}

impl FileLoader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn title(mut self, title: &str) -> Self {
        self.dialog = self.dialog.set_title(title);
        self
    }

    pub fn add_filter(mut self, name: &str, extensions: &[&str]) -> Self {
        self.dialog = self.dialog.add_filter(name, extensions);
        self
    }

    pub fn dispatch(self, tx: Sender<PickedFile>) {
        crate::utils::spawn(async move {
            if let Some(handle) = self.dialog.pick_file().await {
                let data = handle.read().await;
                let _ = tx.send(PickedFile {
                    name: handle.file_name(),
                    data,
                    #[cfg(not(target_arch = "wasm32"))]
                    path: Some(handle.path().to_path_buf()),
                });
            }
        });
    }

    pub fn dispatch_many(self, tx: Sender<Vec<PickedFile>>) {
        crate::utils::spawn(async move {
            let Some(handles) = self.dialog.pick_files().await else {
                return;
            };
            let mut files = Vec::with_capacity(handles.len());
            for handle in handles {
                files.push(PickedFile {
                    name: handle.file_name(),
                    data: handle.read().await,
                    #[cfg(not(target_arch = "wasm32"))]
                    path: Some(handle.path().to_path_buf()),
                });
            }
            let _ = tx.send(files);
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub struct FolderPicker {
    dialog: AsyncFileDialog,
}

#[cfg(not(target_arch = "wasm32"))]
impl FolderPicker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn title(mut self, title: &str) -> Self {
        self.dialog = self.dialog.set_title(title);
        self
    }

    pub fn dispatch(self, tx: Sender<std::path::PathBuf>) {
        crate::utils::spawn(async move {
            if let Some(handle) = self.dialog.pick_folder().await {
                let _ = tx.send(handle.path().to_path_buf());
            }
        });
    }
}

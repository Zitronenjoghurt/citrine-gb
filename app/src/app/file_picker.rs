use std::path::{Path, PathBuf};

/// Describes what a file operation is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileIntent {
    LoadRom,
    LoadBootRom,
    ExportE2E,
}

#[derive(Debug, Clone)]
pub enum FileResultKind {
    File { data: Vec<u8> },
    Directory,
}

#[derive(Debug, Clone)]
pub struct FileResult {
    pub intent: FileIntent,
    pub name: String,
    pub kind: FileResultKind,
    #[cfg(not(target_arch = "wasm32"))]
    pub path: PathBuf,
}

impl FileResult {
    pub fn data(&self) -> Option<&[u8]> {
        match &self.kind {
            FileResultKind::File { data } => Some(data),
            FileResultKind::Directory => None,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn directory_path(&self) -> Option<&Path> {
        match &self.kind {
            FileResultKind::Directory => Some(&self.path),
            FileResultKind::File { .. } => None,
        }
    }
}

struct PendingRequest {
    intent: FileIntent,
    #[cfg(target_arch = "wasm32")]
    promise: poll_promise::Promise<Option<FileResult>>,
}

pub struct FileFilter {
    pub label: &'static str,
    pub extensions: &'static [&'static str],
}

impl FileIntent {
    pub fn filters(&self) -> &[FileFilter] {
        match self {
            Self::LoadRom => &[FileFilter {
                label: "Game Boy ROMs",
                extensions: &["gb", "gbc"],
            }],
            Self::LoadBootRom => &[FileFilter {
                label: "Game Boy Boot ROM",
                extensions: &["bin"],
            }],
            Self::ExportE2E => &[],
        }
    }
}

#[derive(Debug, Clone)]
pub enum SaveResult {
    Success(String),
    Cancelled,
    Error(String),
}

#[derive(Default)]
pub struct FilePicker {
    #[cfg(not(target_arch = "wasm32"))]
    pending_result: Option<FileResult>,
    #[cfg(target_arch = "wasm32")]
    pending: Option<PendingRequest>,
    pending_save: Option<poll_promise::Promise<SaveResult>>,
}

impl FilePicker {
    pub fn poll(&mut self) -> Option<FileResult> {
        #[cfg(not(target_arch = "wasm32"))]
        if self.pending_result.is_some() {
            return self.pending_result.take();
        }

        #[cfg(target_arch = "wasm32")]
        if let Some(pending) = &self.pending {
            if let Some(result) = pending.promise.ready() {
                let result = result.clone();
                self.pending = None;
                return result;
            }
        }

        None
    }

    pub fn poll_save(&mut self) -> Option<SaveResult> {
        if let Some(pending) = &self.pending_save
            && let Some(result) = pending.ready()
        {
            let res = result.clone();
            self.pending_save = None;
            return Some(res);
        }
        None
    }

    pub fn open(&mut self, intent: FileIntent) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            match &intent {
                FileIntent::ExportE2E => {
                    self.pending_result = self.open_directory_blocking(&intent);
                }
                _ => {
                    self.pending_result = self.open_file_blocking(&intent);
                }
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            match &intent {
                FileIntent::ExportE2E => {}
                _ => {
                    let filters: Vec<_> = intent
                        .filters()
                        .iter()
                        .map(|f| (f.label, f.extensions))
                        .collect();

                    let mut dialog = rfd::AsyncFileDialog::new();
                    for (label, exts) in &filters {
                        dialog = dialog.add_filter(*label, exts);
                    }
                    let task = dialog.pick_file();

                    let intent_clone = intent.clone();
                    self.pending = Some(PendingRequest {
                        intent,
                        promise: poll_promise::Promise::spawn_local(async move {
                            let file = task.await?;
                            let data = file.read().await;
                            Some(FileResult {
                                intent: intent_clone,
                                name: file.file_name(),
                                kind: FileResultKind::File { data },
                            })
                        }),
                    });
                }
            }
        }
    }

    pub fn save(&mut self, filename: &str, data: &[u8]) {
        let data = data.to_vec();

        #[cfg(not(target_arch = "wasm32"))]
        {
            let dialog = rfd::FileDialog::new().set_file_name(filename);
            let result = if let Some(path) = dialog.save_file() {
                match std::fs::write(&path, &data) {
                    Ok(_) => {
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        SaveResult::Success(name)
                    }
                    Err(e) => SaveResult::Error(e.to_string()),
                }
            } else {
                SaveResult::Cancelled
            };
            self.pending_save = Some(poll_promise::Promise::from_ready(result));
        }

        #[cfg(target_arch = "wasm32")]
        {
            let dialog = rfd::AsyncFileDialog::new().set_file_name(filename);
            self.pending_save = Some(poll_promise::Promise::spawn_local(async move {
                if let Some(handle) = dialog.save_file().await {
                    match handle.write(&data).await {
                        Ok(_) => SaveResult::Success(handle.file_name()),
                        Err(e) => SaveResult::Error(e.to_string()),
                    }
                } else {
                    SaveResult::Cancelled
                }
            }));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open_file_blocking(&self, intent: &FileIntent) -> Option<FileResult> {
        let mut dialog = rfd::FileDialog::new();
        for filter in intent.filters() {
            dialog = dialog.add_filter(filter.label, filter.extensions);
        }
        let path = dialog.pick_file()?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let data = std::fs::read(&path).ok()?;
        Some(FileResult {
            intent: intent.clone(),
            name,
            kind: FileResultKind::File { data },
            #[cfg(not(target_arch = "wasm32"))]
            path,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open_directory_blocking(&self, intent: &FileIntent) -> Option<FileResult> {
        let path = rfd::FileDialog::new().pick_folder()?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        Some(FileResult {
            intent: intent.clone(),
            name,
            kind: FileResultKind::Directory,
            #[cfg(not(target_arch = "wasm32"))]
            path,
        })
    }
}

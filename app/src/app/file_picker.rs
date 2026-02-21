use egui::DroppedFile;

/// Describes what a file operation is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileIntent {
    LoadRom,
}

#[derive(Debug, Clone)]
pub struct FileResult {
    pub intent: FileIntent,
    pub name: String,
    pub data: Vec<u8>,
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
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::LoadRom => "Drop ROM file here",
        }
    }

    pub fn accepts(&self, filename: &str) -> bool {
        let lower = filename.to_lowercase();
        self.filters()
            .iter()
            .flat_map(|f| f.extensions.iter())
            .any(|ext| lower.ends_with(&format!(".{ext}")))
    }
}

#[derive(Default)]
pub struct FilePicker {
    drop_intent: Option<FileIntent>,
    #[cfg(not(target_arch = "wasm32"))]
    pending_result: Option<FileResult>,
    #[cfg(target_arch = "wasm32")]
    pending: Option<PendingRequest>,
}

impl FilePicker {
    pub fn set_drop_intent(&mut self, intent: impl Into<Option<FileIntent>>) {
        self.drop_intent = intent.into();
    }

    pub fn poll(&mut self, ctx: &egui::Context) -> Option<FileResult> {
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

        self.check_dropped_files(ctx)
    }

    pub fn open(&mut self, intent: FileIntent) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.pending_result = self.open_blocking(&intent);
        }

        #[cfg(target_arch = "wasm32")]
        {
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
                        data,
                    })
                }),
            });
        }
    }

    pub fn save(&self, intent: &FileIntent, filename: &str, data: &[u8]) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut dialog = rfd::FileDialog::new().set_file_name(filename);
            for filter in intent.filters() {
                dialog = dialog.add_filter(filter.label, filter.extensions);
            }
            if let Some(path) = dialog.save_file() {
                let _ = std::fs::write(path, data);
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            let mut dialog = rfd::AsyncFileDialog::new().set_file_name(filename);
            for filter in intent.filters() {
                dialog = dialog.add_filter(filter.label, filter.extensions);
            }
            let data = data.to_vec();
            wasm_bindgen_futures::spawn_local(async move {
                if let Some(handle) = dialog.save_file().await {
                    let _ = handle.write(&data).await;
                }
            });
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open_blocking(&self, intent: &FileIntent) -> Option<FileResult> {
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
            data,
        })
    }

    fn check_dropped_files(&self, ctx: &egui::Context) -> Option<FileResult> {
        let intent = self.drop_intent.as_ref()?;
        let dropped: Vec<DroppedFile> = ctx.input(|i| i.raw.dropped_files.clone());

        for file in dropped {
            let name = file
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| file.name.clone());

            if !intent.accepts(&name) {
                continue;
            }

            let data = if let Some(bytes) = &file.bytes {
                bytes.to_vec()
            } else {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(path) = &file.path {
                    std::fs::read(path).ok()?
                } else {
                    continue;
                }
                #[cfg(target_arch = "wasm32")]
                continue;
            };

            return Some(FileResult {
                intent: intent.clone(),
                name,
                data,
            });
        }

        None
    }

    pub fn show_drop_overlay(&self, ctx: &egui::Context) {
        use egui::{Align2, Color32, Id, LayerId, Order, TextStyle};

        if self.drop_intent.is_none() {
            return;
        }
        if ctx.input(|i| i.raw.hovered_files.is_empty()) {
            return;
        }

        let label = match &self.drop_intent {
            Some(FileIntent::LoadRom) => "Drop ROM file here",
            _ => "Drop file here",
        };

        let painter =
            ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("file_drop_target")));
        let rect = ctx.screen_rect();
        painter.rect_filled(rect, 0.0, Color32::from_black_alpha(192));
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            label,
            TextStyle::Heading.resolve(&ctx.style()),
            Color32::WHITE,
        );
    }
}

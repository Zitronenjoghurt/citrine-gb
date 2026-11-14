#[cfg(not(target_os = "android"))]
use rfd::AsyncFileDialog;
use std::sync::Arc;
#[cfg(not(target_os = "android"))]
use std::sync::mpsc::Sender;

#[cfg(target_os = "android")]
pub use crate::platform::android::files::{FileLoader, FolderPicker};

pub struct PickedFile {
    pub name: String,
    pub data: Arc<[u8]>,
    #[cfg(not(target_arch = "wasm32"))]
    pub path: Option<std::path::PathBuf>,
    pub seen: Option<crate::rom_ids::Seen>,
}

impl PickedFile {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open(path: std::path::PathBuf) -> std::io::Result<Self> {
        let (data, stamp) = crate::rom_ids::read_stamped(&path)?;
        Ok(Self::read(path, data, stamp))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_hashing(
        path: std::path::PathBuf,
        threads: &citrine_gb_threads::Threads,
        known: impl FnOnce(&crate::rom_ids::Seen) -> bool,
    ) -> std::io::Result<(Self, Option<crate::rom_ids::Ids>)> {
        use citrine_gb::console::{ConsoleKind, detect};
        let name = file_name(&path);
        let cached = crate::rom_ids::Stamp::of(&path)
            .is_some_and(|stamp| known(&crate::rom_ids::Seen::at(&path, stamp)));
        let wanted = |head: &[u8]| {
            !crate::utils::zip_games::is_zip(&name, head)
                && matches!(
                    detect(head, Some(&name)),
                    Some(ConsoleKind::Gba | ConsoleKind::Nds)
                )
        };
        let hashing = (!cached).then_some((threads, &wanted as &dyn Fn(&[u8]) -> bool));
        let (data, stamp, ids) = crate::rom_ids::read_stamped_hashing(&path, hashing)?;
        Ok((Self::read(path, data, stamp), ids))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn read(
        path: std::path::PathBuf,
        data: Arc<[u8]>,
        stamp: Option<crate::rom_ids::Stamp>,
    ) -> Self {
        Self {
            name: file_name(&path),
            data,
            seen: stamp.map(|stamp| crate::rom_ids::Seen::at(&path, stamp)),
            path: Some(path),
        }
    }

    pub fn from_drop(file: egui::DroppedFile) -> Option<Self> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = file.path {
            return Self::open(path).ok();
        }
        let data = file.bytes?;
        #[cfg(target_arch = "wasm32")]
        let seen = crate::rom_ids::Seen::file(&file.name, data.len(), file.last_modified);
        #[cfg(not(target_arch = "wasm32"))]
        let seen = None;
        Some(Self {
            name: file.name,
            data,
            #[cfg(not(target_arch = "wasm32"))]
            path: None,
            seen,
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(not(target_os = "android"))]
#[derive(Default)]
pub struct FileLoader {
    dialog: AsyncFileDialog,
}

#[cfg(not(target_os = "android"))]
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

    pub fn dispatch(self, spawner: &citrine_gb_threads::Spawner, tx: Sender<PickedFile>) {
        self.pick(spawner, move |file| {
            let _ = tx.send(file);
        });
    }

    pub fn pick(
        self,
        spawner: &citrine_gb_threads::Spawner,
        done: impl FnOnce(PickedFile) + Send + 'static,
    ) {
        spawner.run_future("file dialog", async move {
            if let Some(handle) = self.dialog.pick_file().await {
                done(picked(handle).await);
            }
        });
    }

    pub fn dispatch_many(self, spawner: &citrine_gb_threads::Spawner, tx: Sender<Vec<PickedFile>>) {
        spawner.run_future("file dialog", async move {
            let Some(handles) = self.dialog.pick_files().await else {
                return;
            };
            let mut files = Vec::with_capacity(handles.len());
            for handle in handles {
                files.push(picked(handle).await);
            }
            let _ = tx.send(files);
        });
    }
}

#[cfg(not(target_os = "android"))]
async fn picked(handle: rfd::FileHandle) -> PickedFile {
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(file) = PickedFile::open(handle.path().to_path_buf()) {
        return file;
    }
    let name = handle.file_name();
    #[cfg(target_arch = "wasm32")]
    let data = read_shared(handle.inner()).await;
    #[cfg(not(target_arch = "wasm32"))]
    let data = Arc::from(handle.read().await);
    #[cfg(target_arch = "wasm32")]
    let seen = {
        let modified = std::time::Duration::from_millis(handle.inner().last_modified() as u64);
        crate::rom_ids::Seen::file(&name, data.len(), Some(std::time::UNIX_EPOCH + modified))
    };
    #[cfg(not(target_arch = "wasm32"))]
    let seen = None;
    PickedFile {
        name,
        data,
        #[cfg(not(target_arch = "wasm32"))]
        path: Some(handle.path().to_path_buf()),
        seen,
    }
}

#[cfg(target_arch = "wasm32")]
async fn read_shared(file: &web_sys::File) -> Arc<[u8]> {
    let Ok(buffer) = wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await else {
        return Arc::new([]);
    };
    shared_bytes(&js_sys::Uint8Array::new(&buffer))
}

#[cfg(target_arch = "wasm32")]
pub fn shared_bytes(array: &js_sys::Uint8Array) -> Arc<[u8]> {
    let mut data = Arc::<[u8]>::new_uninit_slice(array.length() as usize);
    array.copy_to_uninit(Arc::get_mut(&mut data).expect("a new slice has one owner"));
    unsafe { data.assume_init() }
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
#[derive(Default)]
pub struct FolderPicker {
    dialog: AsyncFileDialog,
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
impl FolderPicker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn title(mut self, title: &str) -> Self {
        self.dialog = self.dialog.set_title(title);
        self
    }

    pub fn dispatch(self, spawner: &citrine_gb_threads::Spawner, tx: Sender<std::path::PathBuf>) {
        self.pick(spawner, move |path| {
            let _ = tx.send(path);
        });
    }

    pub fn pick(
        self,
        spawner: &citrine_gb_threads::Spawner,
        done: impl FnOnce(std::path::PathBuf) + Send + 'static,
    ) {
        spawner.run_future("file dialog", async move {
            if let Some(handle) = self.dialog.pick_folder().await {
                done(handle.path().to_path_buf());
            }
        });
    }
}

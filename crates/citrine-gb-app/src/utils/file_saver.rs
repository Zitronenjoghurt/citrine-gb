use rfd::AsyncFileDialog;
use std::sync::mpsc::Sender;

pub enum SaveOutcome {
    Saved(String),
    SavedPair(String, String),
    Failed(String),
    Cancelled,
}

pub struct FileSaver {
    dialog: AsyncFileDialog,
    #[cfg(target_arch = "wasm32")]
    name: String,
}

impl FileSaver {
    pub fn new(file_name: &str) -> Self {
        Self {
            dialog: AsyncFileDialog::new().set_file_name(file_name),
            #[cfg(target_arch = "wasm32")]
            name: file_name.to_string(),
        }
    }

    pub fn add_filter(mut self, name: &str, extensions: &[&str]) -> Self {
        self.dialog = self.dialog.add_filter(name, extensions);
        self
    }

    pub fn dispatch(self, data: Vec<u8>, tx: Sender<SaveOutcome>) {
        #[cfg(not(target_arch = "wasm32"))]
        crate::utils::spawn(async move {
            let outcome = match self.dialog.save_file().await {
                Some(handle) => match handle.write(&data).await {
                    Ok(()) => SaveOutcome::Saved(handle.file_name()),
                    Err(err) => SaveOutcome::Failed(err.to_string()),
                },
                None => SaveOutcome::Cancelled,
            };
            let _ = tx.send(outcome);
        });

        #[cfg(target_arch = "wasm32")]
        {
            let outcome = match trigger_download(&self.name, &data) {
                Ok(()) => SaveOutcome::Saved(self.name.clone()),
                Err(err) => SaveOutcome::Failed(err),
            };
            let _ = tx.send(outcome);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn dispatch_copy(self, source: std::path::PathBuf, tx: Sender<SaveOutcome>) {
        crate::utils::spawn(async move {
            let outcome = match self.dialog.save_file().await {
                Some(handle) => match copy_unless_same(&source, handle.path()) {
                    Ok(()) => SaveOutcome::Saved(handle.file_name()),
                    Err(err) => SaveOutcome::Failed(err.to_string()),
                },
                None => SaveOutcome::Cancelled,
            };
            let _ = tx.send(outcome);
        });
    }

    #[cfg(target_arch = "wasm32")]
    pub fn dispatch_blob(self, blob: &web_sys::Blob, tx: Sender<SaveOutcome>) {
        let outcome = match download_blob(&self.name, blob) {
            Ok(()) => SaveOutcome::Saved(self.name.clone()),
            Err(err) => SaveOutcome::Failed(err),
        };
        let _ = tx.send(outcome);
    }

    pub fn dispatch_pair(
        self,
        data: Vec<u8>,
        companion_extension: &'static str,
        companion: Vec<u8>,
        tx: Sender<SaveOutcome>,
    ) {
        #[cfg(not(target_arch = "wasm32"))]
        crate::utils::spawn(async move {
            let outcome = match self.dialog.save_file().await {
                Some(handle) => {
                    let companion_path = handle.path().with_extension(companion_extension);
                    let companion_name = companion_path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    match handle
                        .write(&data)
                        .await
                        .and_then(|()| std::fs::write(&companion_path, &companion))
                    {
                        Ok(()) => SaveOutcome::SavedPair(handle.file_name(), companion_name),
                        Err(err) => SaveOutcome::Failed(err.to_string()),
                    }
                }
                None => SaveOutcome::Cancelled,
            };
            let _ = tx.send(outcome);
        });

        #[cfg(target_arch = "wasm32")]
        {
            let companion_name = std::path::Path::new(&self.name)
                .with_extension(companion_extension)
                .to_string_lossy()
                .into_owned();
            let outcome = match trigger_download(&self.name, &data)
                .and_then(|()| trigger_download(&companion_name, &companion))
            {
                Ok(()) => SaveOutcome::SavedPair(self.name.clone(), companion_name),
                Err(err) => SaveOutcome::Failed(err),
            };
            let _ = tx.send(outcome);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn copy_unless_same(source: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    let same = match (source.canonicalize(), target.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    if !same {
        std::fs::copy(source, target)?;
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn trigger_download(file_name: &str, data: &[u8]) -> Result<(), String> {
    let bytes = js_sys::Uint8Array::from(data);
    let parts = js_sys::Array::new();
    parts.push(&bytes.buffer());

    let blob = web_sys::Blob::new_with_buffer_source_sequence(&parts).map_err(|_| "blob failed")?;
    download_blob(file_name, &blob)
}

#[cfg(target_arch = "wasm32")]
fn download_blob(file_name: &str, blob: &web_sys::Blob) -> Result<(), String> {
    use web_sys::wasm_bindgen::JsCast;
    use web_sys::{HtmlAnchorElement, Url};

    let url = Url::create_object_url_with_blob(blob).map_err(|_| "object url failed")?;

    let document = web_sys::window()
        .and_then(|w| w.document())
        .ok_or("no document")?;
    let body = document.body().ok_or("no body")?;
    let anchor = document
        .create_element("a")
        .map_err(|_| "create element failed")?
        .dyn_into::<HtmlAnchorElement>()
        .map_err(|_| "not an anchor")?;

    anchor.set_href(&url);
    anchor.set_download(file_name);
    body.append_child(&anchor).map_err(|_| "append failed")?;
    anchor.click();
    let _ = body.remove_child(&anchor);
    let _ = Url::revoke_object_url(&url);
    Ok(())
}

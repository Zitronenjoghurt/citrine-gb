use crate::utils::file_loader::PickedFile;
use std::sync::mpsc::{Receiver, Sender, channel};

#[cfg(not(target_os = "android"))]
const FOLDER_DEPTH: usize = 2;

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct RomFolder {
    pub tree: Option<String>,
    #[serde(skip)]
    pub games: Option<Vec<FolderGame>>,
    #[serde(skip)]
    pub readable: bool,
    #[serde(skip)]
    pub search: String,
    #[serde(skip)]
    pub searching: bool,
    #[serde(skip)]
    channels: Channels,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FolderGame {
    pub name: String,
    pub uri: String,
}

pub fn game_name(file: &str) -> String {
    let lower = file.to_lowercase();
    let cut = [".gbc", ".gb"]
        .iter()
        .find(|extension| lower.ends_with(*extension))
        .map_or(file.len(), |extension| file.len() - extension.len());
    file[..cut].trim().to_owned()
}

pub fn folder_label(tree: &str) -> String {
    if !tree.starts_with("content://") {
        return std::path::Path::new(tree).file_name().map_or_else(
            || tree.to_owned(),
            |name| name.to_string_lossy().into_owned(),
        );
    }
    let last = tree.rsplit('/').next().unwrap_or(tree);
    let decoded = percent_decode(last);
    match decoded.split_once(':') {
        Some((_, "")) => "Phone storage".to_owned(),
        Some((_, path)) => path.to_owned(),
        None => decoded,
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = bytes
            .get(index + 1..index + 3)
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        match (bytes[index], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                index += 3;
            }
            (byte, _) => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn matching<'a>(games: &'a [FolderGame], search: &str) -> Vec<&'a FolderGame> {
    let search = search.trim().to_lowercase();
    games
        .iter()
        .filter(|game| search.is_empty() || game.name.to_lowercase().contains(&search))
        .collect()
}

pub struct Listing {
    pub tree: String,
    pub games: Vec<(String, String)>,
    pub readable: bool,
}

struct Channels {
    tree_tx: Sender<String>,
    tree_rx: Receiver<String>,
    list_tx: Sender<Listing>,
    list_rx: Receiver<Listing>,
    asked: bool,
}

impl Default for Channels {
    fn default() -> Self {
        let (tree_tx, tree_rx) = channel();
        let (list_tx, list_rx) = channel();
        Self {
            tree_tx,
            tree_rx,
            list_tx,
            list_rx,
            asked: false,
        }
    }
}

impl RomFolder {
    pub fn label(&self) -> Option<String> {
        self.tree.as_deref().map(folder_label)
    }

    pub fn choose(&self) {
        #[cfg(target_os = "android")]
        crate::platform::android::files::pick_rom_folder(self.channels.tree_tx.clone());
        #[cfg(not(target_os = "android"))]
        {
            let (tx, rx) = channel();
            let trees = self.channels.tree_tx.clone();
            crate::utils::file_loader::FolderPicker::new()
                .title("ROM folder")
                .dispatch(tx);
            std::thread::spawn(move || {
                if let Ok(path) = rx.recv() {
                    let _ = trees.send(path.to_string_lossy().into_owned());
                }
            });
        }
    }

    pub fn refresh(&mut self) {
        let Some(tree) = self.tree.clone() else {
            return;
        };
        self.channels.asked = true;
        let tx = self.channels.list_tx.clone();
        #[cfg(target_os = "android")]
        crate::platform::android::files::list_roms(&tree, tx);
        #[cfg(not(target_os = "android"))]
        std::thread::spawn(move || {
            let _ = tx.send(list_dir(&tree));
        });
    }

    pub fn poll(&mut self) {
        while let Ok(tree) = self.channels.tree_rx.try_recv() {
            self.tree = Some(tree);
            self.games = None;
            self.refresh();
        }
        if self.tree.is_some() && !self.channels.asked {
            self.refresh();
        }
        while let Ok(listing) = self.channels.list_rx.try_recv() {
            if Some(&listing.tree) != self.tree.as_ref() {
                continue;
            }
            self.readable = listing.readable;
            self.games = Some(
                listing
                    .games
                    .into_iter()
                    .map(|(file, uri)| FolderGame {
                        name: game_name(&file),
                        uri,
                    })
                    .collect(),
            );
        }
    }

    pub fn read(&self, game: &FolderGame, tx: Sender<PickedFile>) {
        #[cfg(target_os = "android")]
        crate::platform::android::files::read_rom(&game.uri, tx);
        #[cfg(not(target_os = "android"))]
        {
            let path = std::path::PathBuf::from(&game.uri);
            std::thread::spawn(move || {
                if let Ok(data) = std::fs::read(&path) {
                    let name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let _ = tx.send(PickedFile {
                        name,
                        data,
                        path: Some(path),
                    });
                }
            });
        }
    }
}

#[cfg(not(target_os = "android"))]
pub fn list_dir(tree: &str) -> Listing {
    let mut games = Vec::new();
    let readable = collect(std::path::Path::new(tree), FOLDER_DEPTH, &mut games).is_ok();
    games.sort_by_key(|(name, _)| name.to_lowercase());
    Listing {
        tree: tree.to_owned(),
        games,
        readable,
    }
}

#[cfg(not(target_os = "android"))]
fn collect(
    dir: &std::path::Path,
    depth: usize,
    games: &mut Vec<(String, String)>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if depth > 0 {
                let _ = collect(&path, depth - 1, games);
            }
        } else if game_name(&name) != name {
            games.push((name, path.to_string_lossy().into_owned()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

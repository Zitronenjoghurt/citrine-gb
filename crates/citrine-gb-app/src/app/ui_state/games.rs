use super::library::{Filter, LibraryGame};

const UNDO_SECONDS: f64 = 8.0;

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct GamesPane {
    pub hidden: bool,
    pub share: Option<f32>,
    pub filter: Filter,
    #[serde(skip)]
    pub search: String,
    #[serde(skip)]
    pub gone: Vec<std::path::PathBuf>,
    #[serde(skip)]
    pub checked: Option<f64>,
    #[serde(skip)]
    pub page: usize,
    #[serde(skip)]
    pub grid_columns: usize,
    #[serde(skip)]
    pub undo: Option<Undo>,
    #[serde(skip)]
    pub focused: bool,
    #[serde(skip)]
    pub info: Option<String>,
    #[serde(skip)]
    pub tag_draft: Option<String>,
    #[serde(skip)]
    pub tag_search: String,
    #[serde(skip)]
    pub library_shown: bool,
}

pub struct Undo {
    pub folder: Option<String>,
    pub games: Vec<LibraryGame>,
    pub words: String,
    shown: Option<f64>,
}

impl Undo {
    pub fn new(folder: Option<String>, games: Vec<LibraryGame>) -> Self {
        let words = match (&folder, games.as_slice()) {
            (Some(tree), _) => format!("Removed {}", super::library::scan::folder_label(tree)),
            (None, [game]) => format!("Removed {}", game.name),
            (None, games) => format!("Removed {} games", games.len()),
        };
        Self {
            folder,
            games,
            words,
            shown: None,
        }
    }
}

impl GamesPane {
    pub fn open_info(&mut self, path: String) {
        if self.info.as_ref() != Some(&path) {
            self.tag_draft = None;
        }
        self.info = Some(path);
    }

    pub fn close_info(&mut self) {
        self.info = None;
        self.tag_draft = None;
    }

    pub fn offer_undo(&mut self, undo: Undo) -> Option<Undo> {
        self.undo.replace(undo)
    }

    pub fn expired_undo(&mut self, now: f64) -> Option<Undo> {
        let undo = self.undo.as_mut()?;
        let shown = *undo.shown.get_or_insert(now);
        if now - shown < UNDO_SECONDS {
            return None;
        }
        self.undo.take()
    }
}

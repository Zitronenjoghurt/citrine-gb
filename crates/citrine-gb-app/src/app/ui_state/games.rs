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
    pub download: Option<String>,
    pub name: String,
    pub tail: String,
    shown: Option<f64>,
}

impl Undo {
    pub fn new(folder: Option<String>, games: Vec<LibraryGame>) -> Self {
        let name = match (&folder, games.as_slice()) {
            (Some(tree), _) => super::library::scan::folder_label(tree),
            (None, [game]) => game.name.clone(),
            (None, games) => format!("{} games", games.len()),
        };
        Self {
            folder,
            games,
            download: None,
            name,
            tail: " removed".to_owned(),
            shown: None,
        }
    }

    pub fn download(path: String, name: &str) -> Self {
        Self {
            folder: None,
            games: Vec::new(),
            download: Some(path),
            name: name.to_owned(),
            tail: " download deleted".to_owned(),
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

    pub fn pending_download(&self) -> Option<&str> {
        self.undo.as_ref()?.download.as_deref()
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

#[cfg(test)]
mod tests {
    use super::{LibraryGame, Undo};

    #[test]
    fn a_removal_undo_names_the_game_first() {
        let game = LibraryGame {
            name: "Alleyway".to_owned(),
            ..LibraryGame::default()
        };
        let undo = Undo::new(None, vec![game.clone()]);
        assert_eq!(format!("{}{}", undo.name, undo.tail), "Alleyway removed");
        let undo = Undo::new(None, vec![game.clone(), game]);
        assert_eq!(format!("{}{}", undo.name, undo.tail), "2 games removed");
    }
}

use crate::app::tabs::{Tab, TabViewer};
use crate::utils::file_loader::{FileLoader, PickedFile};
use citrine_gb_nds_files::{FileTree, FileType, Group, NdsRom, NodeId, Query, Row};
use egui::{Color32, Ui};
use std::collections::HashSet;
use std::sync::{Arc, mpsc};

pub(crate) mod explorer;
mod file;
#[cfg(feature = "nds")]
mod model;
#[cfg(feature = "nds")]
mod sound;
mod textures;
mod tiles;

#[cfg(feature = "nds")]
const GAME_CHECK_SECONDS: f64 = 1.0;

pub struct Cart {
    name: String,
    bytes: Arc<[u8]>,
    tree: FileTree,
    from_game: bool,
}

impl Cart {
    pub fn read(name: String, bytes: Arc<[u8]>, from_game: bool) -> Result<Self, String> {
        let rom = NdsRom::parse(&bytes).map_err(|err| err.to_string())?;
        let tree = FileTree::read(&rom).map_err(|err| err.to_string())?;
        Ok(Self {
            name,
            bytes,
            tree,
            from_game,
        })
    }

    pub fn tree(&self) -> &FileTree {
        &self.tree
    }

    fn file(&self, node: NodeId) -> Option<Arc<[u8]>> {
        let rom = NdsRom::parse(&self.bytes).ok()?;
        self.tree.bytes(&rom, node).ok().map(Arc::from)
    }
}

pub struct OpenFile {
    cart: Arc<Cart>,
    node: NodeId,
    bytes: Arc<[u8]>,
    view: file::FileView,
}

impl OpenFile {
    pub fn title(&self) -> String {
        let tree = &self.cart.tree;
        let name = tree.node(self.node).name();
        match tree.parent(self.node) {
            Some(parent) if tree.node(parent).file_type() == Some(FileType::Narc) => {
                let narc = tree.node(parent).name();
                let stem = narc.rsplit_once('.').map_or(narc, |(stem, _)| stem);
                format!("{stem}/{name}")
            }
            _ => name.to_owned(),
        }
    }
}

pub struct DsFilesUi {
    carts: Vec<Arc<Cart>>,
    current: usize,
    query: Query,
    toggled: HashSet<NodeId>,
    rows: Option<Vec<Row>>,
    count: usize,
    selected: Option<NodeId>,
    folder: Option<NodeId>,
    files: Vec<Option<OpenFile>>,
    picked_tx: mpsc::Sender<PickedFile>,
    picked_rx: mpsc::Receiver<PickedFile>,
    problem: Option<String>,
    #[cfg(feature = "nds")]
    game_checked: Option<f64>,
    filter_sheet: bool,
}

impl Default for DsFilesUi {
    fn default() -> Self {
        let (picked_tx, picked_rx) = mpsc::channel();
        Self {
            carts: Vec::new(),
            current: 0,
            query: Query::default(),
            toggled: HashSet::new(),
            rows: None,
            count: 0,
            selected: None,
            folder: None,
            files: Vec::new(),
            picked_tx,
            picked_rx,
            problem: None,
            #[cfg(feature = "nds")]
            game_checked: None,
            filter_sheet: false,
        }
    }
}

impl DsFilesUi {
    pub fn cart(&self) -> Option<&Arc<Cart>> {
        self.carts.get(self.current)
    }

    pub fn add_cart(&mut self, cart: Cart) {
        if cart.from_game {
            self.carts.retain(|old| !old.from_game);
        }
        self.carts.push(Arc::new(cart));
        self.choose(self.carts.len() - 1);
    }

    fn choose(&mut self, index: usize) {
        self.current = index;
        self.toggled.clear();
        self.rows = None;
        self.selected = None;
        self.folder = None;
    }

    fn changed(&mut self) {
        self.rows = None;
    }

    fn set_query_text(&mut self, text: String) {
        if text != self.query.text {
            self.query.text = text;
            self.toggled.clear();
            self.changed();
        }
    }

    fn toggle_group(&mut self, group: Group) {
        if !self.query.groups.remove(&group) {
            self.query.groups.insert(group);
        }
        self.toggled.clear();
        self.changed();
    }

    fn toggle_open(&mut self, node: NodeId) {
        if !self.toggled.remove(&node) {
            self.toggled.insert(node);
        }
        self.changed();
    }

    fn rows(&mut self) -> &[Row] {
        if self.rows.is_none() {
            let (rows, count) = match self.carts.get(self.current) {
                Some(cart) => (
                    cart.tree.rows(&self.query, &self.toggled),
                    cart.tree.count(&self.query),
                ),
                None => (Vec::new(), 0),
            };
            self.rows = Some(rows);
            self.count = count;
        }
        self.rows.as_deref().unwrap_or_default()
    }

    pub fn open(&mut self, node: NodeId) -> Option<u32> {
        let cart = self.cart()?.clone();
        let found = self.files.iter().position(|file| {
            file.as_ref()
                .is_some_and(|file| file.node == node && Arc::ptr_eq(&file.cart, &cart))
        });
        if let Some(slot) = found {
            return Some(slot as u32);
        }
        let bytes = cart.file(node)?;
        let view = file::FileView::new(&cart, node, cart.tree.node(node).file_type(), &bytes);
        let file = OpenFile {
            cart,
            node,
            bytes,
            view,
        };
        let slot = match self.files.iter().position(Option::is_none) {
            Some(slot) => {
                self.files[slot] = Some(file);
                slot
            }
            None => {
                self.files.push(Some(file));
                self.files.len() - 1
            }
        };
        Some(slot as u32)
    }

    pub fn close(&mut self, slot: u32, emulator: &mut crate::emulator::Emulator) {
        if let Some(file) = self.files.get_mut(slot as usize) {
            if let Some(open) = file {
                open.view.stop(emulator);
            }
            *file = None;
        }
    }

    pub fn file(&self, slot: u32) -> Option<&OpenFile> {
        self.files.get(slot as usize)?.as_ref()
    }

    pub fn title(&self, slot: u32) -> String {
        self.file(slot)
            .map_or_else(|| "File".to_owned(), OpenFile::title)
    }

    pub fn folder(&self) -> Option<NodeId> {
        self.folder
    }

    pub fn folder_name(&self) -> Option<&str> {
        let node = self.folder?;
        Some(self.cart()?.tree.node(node).name())
    }

    pub fn folder_up(&mut self) -> bool {
        let Some(node) = self.folder else {
            return false;
        };
        self.folder = self
            .cart()
            .and_then(|cart| cart.tree.parent(node))
            .filter(|&p| p != 0);
        true
    }

    fn take_picked(&mut self) {
        while let Ok(picked) = self.picked_rx.try_recv() {
            let name = picked
                .name
                .rsplit_once('.')
                .map_or(picked.name.as_str(), |(stem, _)| stem)
                .to_owned();
            match Cart::read(name, picked.data, false) {
                Ok(cart) => {
                    self.problem = None;
                    self.add_cart(cart);
                }
                Err(_) => {
                    self.problem = Some(format!("{} isn't a DS game.", picked.name));
                }
            }
        }
    }

    #[cfg(feature = "nds")]
    fn check_game(&mut self, emulator: &crate::emulator::Emulator, now: f64) {
        if self
            .game_checked
            .is_some_and(|at| now - at < GAME_CHECK_SECONDS)
        {
            return;
        }
        self.game_checked = Some(now);
        if !emulator.has_game() || emulator.kind() != citrine_gb::console::ConsoleKind::Nds {
            return;
        }
        let bytes = emulator.rom_bytes();
        if self
            .carts
            .iter()
            .any(|cart| Arc::ptr_eq(&cart.bytes, &bytes))
        {
            return;
        }
        let name = crate::ds_banner::title(&bytes)
            .and_then(|title| title.lines().next().map(str::to_owned))
            .unwrap_or_else(|| "Running game".to_owned());
        let showing_game = self.cart().is_none_or(|cart| cart.from_game);
        if let Ok(cart) = Cart::read(name, bytes, true) {
            let current = self.cart().cloned();
            self.add_cart(cart);
            if !showing_game
                && let Some(index) = self
                    .carts
                    .iter()
                    .position(|cart| current.as_ref().is_some_and(|c| Arc::ptr_eq(c, cart)))
            {
                self.choose(index);
            }
        }
    }

    fn pick_file(&self, spawner: &citrine_gb_threads::Spawner) {
        FileLoader::new()
            .title("Open a DS game")
            .add_filter("DS game", &["nds", "srl", "dsi"])
            .dispatch(spawner, self.picked_tx.clone());
    }
}

pub fn group_color(group: Group, dark: bool) -> Color32 {
    let (light, night) = match group {
        Group::Pictures => ((0x8b, 0x5c, 0xf6), (0xa9, 0x8b, 0xff)),
        Group::Models => ((0x2f, 0x80, 0xd6), (0x5a, 0xa7, 0xf0)),
        Group::Animations => ((0x0f, 0x9f, 0x8f), (0x34, 0xc3, 0xb2)),
        Group::Sound => ((0xe0, 0x7b, 0x1a), (0xf3, 0x9a, 0x45)),
        Group::Archives => ((0xb8, 0x90, 0x1a), (0xe0, 0xb4, 0x3a)),
        Group::Text => ((0x3f, 0x9a, 0x3f), (0x6c, 0xc3, 0x6c)),
        Group::Other => ((0x8a, 0x8a, 0x8a), (0x9a, 0x9a, 0x9a)),
    };
    let (r, g, b) = if dark { night } else { light };
    Color32::from_rgb(r, g, b)
}

pub fn group_icon(group: Group) -> &'static str {
    use crate::icons;
    match group {
        Group::Pictures => icons::IMAGE,
        Group::Models => icons::CUBE,
        Group::Animations => icons::FILM_STRIP,
        Group::Sound => icons::MUSIC_NOTES,
        Group::Archives => icons::ARCHIVE,
        Group::Text => icons::FILE_TEXT,
        Group::Other => icons::FILE,
    }
}

pub fn size_text(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{:.1} KB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

pub fn show(viewer: &mut TabViewer, ui: &mut Ui) {
    let state = &mut viewer.ui.ds_files;
    state.take_picked();
    #[cfg(feature = "nds")]
    state.check_game(viewer.emulator, ui.input(|i| i.time));
    let phone = viewer.ui.settings.layout.is_touch(ui.ctx());
    let opened = explorer::show(&mut viewer.ui.ds_files, ui, phone, &viewer.files.spawner);
    if let Some(node) = opened
        && let Some(slot) = viewer.ui.ds_files.open(node)
    {
        viewer.events.open_tab(Tab::DsFile(slot));
    }
}

pub fn show_file(viewer: &mut TabViewer, ui: &mut Ui, slot: u32) {
    let phone = viewer.ui.settings.layout.is_touch(ui.ctx());
    let Some(Some(open)) = viewer.ui.ds_files.files.get_mut(slot as usize) else {
        ui.weak("This file is closed.");
        return;
    };
    file::show(open, ui, phone, viewer.files, viewer.emulator);
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb_nds_files::fixtures::{narc, nds};

    fn cart() -> Cart {
        let textures = narc::build(&[b"BTX0\xFF\xFE".to_vec(), b"RGCN\xFF\xFE".to_vec()]);
        let bytes = nds::build(
            b"CPUE",
            &[
                ("fielddata/map_tex_set.narc", textures),
                ("data/intro.txt", b"Hello\n".to_vec()),
            ],
        );
        Cart::read("Test".into(), Arc::from(bytes), false).unwrap()
    }

    #[test]
    fn opening_a_file_twice_gives_the_same_slot() {
        let mut state = DsFilesUi::default();
        state.add_cart(cart());
        let node = state
            .cart()
            .unwrap()
            .tree
            .find(&["data", "intro.txt"])
            .unwrap();
        let slot = state.open(node).unwrap();
        assert_eq!(state.open(node), Some(slot));
        assert_eq!(state.file(slot).unwrap().bytes.as_ref(), b"Hello\n");
    }

    #[test]
    fn a_closed_slot_is_reused() {
        let mut state = DsFilesUi::default();
        state.add_cart(cart());
        let tree = state.cart().unwrap().tree.clone();
        let first = state
            .open(tree.find(&["data", "intro.txt"]).unwrap())
            .unwrap();
        let second = state
            .open(tree.find(&["fielddata", "map_tex_set.narc", "1"]).unwrap())
            .unwrap();
        assert_ne!(first, second);
        state.close(first, &mut crate::emulator::Emulator::default());
        let third = state
            .open(tree.find(&["fielddata", "map_tex_set.narc", "0"]).unwrap())
            .unwrap();
        assert_eq!(third, first);
    }

    #[test]
    fn narc_members_are_titled_by_archive_and_index() {
        let mut state = DsFilesUi::default();
        state.add_cart(cart());
        let tree = state.cart().unwrap().tree.clone();
        let slot = state
            .open(tree.find(&["fielddata", "map_tex_set.narc", "1"]).unwrap())
            .unwrap();
        assert_eq!(state.title(slot), "map_tex_set/1");
        let text = state
            .open(tree.find(&["data", "intro.txt"]).unwrap())
            .unwrap();
        assert_eq!(state.title(text), "intro.txt");
    }

    #[test]
    fn a_new_game_cart_replaces_the_old_one() {
        let mut state = DsFilesUi::default();
        state.add_cart(cart());
        let mut game = cart();
        game.from_game = true;
        state.add_cart(game);
        let mut next = cart();
        next.from_game = true;
        state.add_cart(next);
        assert_eq!(state.carts.len(), 2);
        assert!(state.cart().unwrap().from_game);
    }

    #[test]
    fn a_file_that_is_no_ds_game_is_named_in_the_problem() {
        let mut state = DsFilesUi::default();
        state
            .picked_tx
            .send(PickedFile {
                name: "notes.txt".into(),
                data: b"hello"[..].into(),
                #[cfg(not(target_arch = "wasm32"))]
                path: None,
                seen: None,
            })
            .unwrap();
        state.take_picked();
        assert_eq!(state.problem.as_deref(), Some("notes.txt isn't a DS game."));
        assert!(state.cart().is_none());
    }

    #[test]
    fn searching_counts_matches_and_resets_closed_folders() {
        let mut state = DsFilesUi::default();
        state.add_cart(cart());
        state.toggle_open(1);
        state.set_query_text("map".into());
        assert!(state.toggled.is_empty());
        let rows = state.rows().len();
        assert_eq!(rows, 4);
        assert_eq!(state.count, 3);
    }
}

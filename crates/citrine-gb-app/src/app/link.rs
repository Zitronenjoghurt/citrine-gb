use crate::app::Citrine;
use crate::app::tabs::Tab;
use crate::app::ui_state::settings::SettingsTab;
use crate::local_link::Game;
use crate::partner_link::PartnerLink;
use crate::utils::file_loader::PickedFile;
use citrine_gb::console::ConsoleKind;
use citrine_gb::rom::Rom;

const ONLY_EACH_OTHER: &str = "Game Boy Advance games link only with each other";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkEntry {
    Ready,
    Open,
    Blocked(&'static str),
}

impl Citrine {
    pub(super) fn link_entry(&self) -> LinkEntry {
        if self.emulator.link.is_some() || self.emulator.partner.is_some() {
            LinkEntry::Open
        } else if self.play.is_live() {
            LinkEntry::Blocked("Not while you play together")
        } else if !self.emulator.has_game() {
            LinkEntry::Blocked("Open a game first")
        } else {
            LinkEntry::Ready
        }
    }

    pub(super) fn pick_second_game(&mut self, touch: bool) {
        match self.link_entry() {
            LinkEntry::Blocked(why) => {
                self.toasts.error(why);
            }
            LinkEntry::Ready => {
                self.ui.linked.picking = true;
                self.show_tab(Tab::Playing, touch);
            }
            LinkEntry::Open => self.show_tab(Tab::Playing, touch),
        }
    }

    pub(super) fn link_picked(&mut self, file: PickedFile) {
        #[cfg(not(target_arch = "wasm32"))]
        let path = file.path.clone();
        let linked = self.link_game(file.data, Some(file.name));
        #[cfg(not(target_arch = "wasm32"))]
        if linked
            && let Some(path) = path
            && let Some(link) = &self.emulator.link
        {
            let (title, kind) = (link.title().to_owned(), link.kind());
            let dropped = self.ui.recent.record_on(&path, &title, None, Some(kind));
            self.files
                .rom_copies
                .drop_copies(dropped.iter().map(|entry| entry.path.as_path()));
        }
        #[cfg(target_arch = "wasm32")]
        let _ = linked;
    }

    pub(super) fn start_partner(&mut self, event: &'static citrine_gb_partner::Event) {
        let Some(rom) = self.emulator.as_gb().map(|gb| gb.cartridge.rom_bytes()) else {
            return;
        };
        let Some(game) = citrine_gb_gamedata::identify(&rom) else {
            self.toasts.error("This game has no event partner");
            return;
        };
        let seed = web_time::SystemTime::now()
            .duration_since(web_time::SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        let partner = match PartnerLink::start(game, &rom, event, seed) {
            Ok(partner) => partner,
            Err(err) => {
                self.toasts
                    .error(format!("Couldn't start the event partner: {err}"));
                return;
            }
        };
        if let Err(err) = self.emulator.start_partner(partner) {
            self.toasts
                .error(format!("Couldn't save the second game: {err}"));
            return;
        }
        self.try_start_audio();
        self.ui.linked.picking = false;
        self.ui.linked.listing = false;
        self.ui.linked.from_list = true;
        if self.mobile.phone {
            self.close_mobile_sheet();
        } else {
            self.open_tab(Tab::Playing);
        }
    }

    pub(super) fn link_game(&mut self, data: Vec<u8>, name: Option<String>) -> bool {
        let rom = Rom::new(&data);
        let kind = self.opened_as(&rom, name.as_deref());
        if (kind == ConsoleKind::Gba) != (self.emulator.kind() == ConsoleKind::Gba) {
            self.toasts.error(ONLY_EACH_OTHER);
            return false;
        }
        self.try_start_audio();
        if kind != ConsoleKind::Gba && rom.header().is_err() {
            self.toasts.error("That file isn't a Game Boy game");
            return false;
        }
        if let Err(err) = self.emulator.start_link(&rom, kind, name.as_deref()) {
            self.toasts
                .error(format!("Couldn't start the second game: {err}"));
            return false;
        }
        self.ui.linked.picking = false;
        self.ui.settings.dirty = true;
        if let Some(link) = &mut self.emulator.link {
            link.set_settings(&self.ui.settings.local_link);
        }
        if self.mobile.phone {
            self.close_mobile_sheet();
        } else {
            self.open_tab(Tab::Playing);
        }
        true
    }

    pub(super) fn end_link(&mut self) {
        self.ui.linked.picking = false;
        self.ui.linked.listing = false;
        self.ui.linked.from_list = false;
        if let Err(err) = self.emulator.end_link() {
            self.toasts
                .error(format!("Couldn't save the second game: {err}"));
        }
        if let Some(location) = self.dock.find_tab(&Tab::Playing)
            && self.play.session().is_none()
        {
            self.dock.remove_tab(location);
        }
        if self.mobile.showing(Tab::Playing) {
            self.close_mobile_sheet();
        }
    }

    pub(super) fn open_settings(&mut self, tab: SettingsTab, touch: bool) {
        self.ui.settings.current_tab = tab;
        if touch {
            self.mobile.nav.open_settings(tab);
            self.release_touch();
        } else {
            self.open_tab(Tab::Settings);
        }
    }

    pub(super) fn keeps_link_tab(&self) -> bool {
        self.emulator.link.is_some()
            || self.emulator.partner.is_some()
            || self.ui.linked.picking
            || self.ui.linked.listing
    }

    pub(super) fn switch_game(&mut self) {
        let Some(link) = &self.emulator.link else {
            self.toasts.error("No second game is linked");
            return;
        };
        let next = link.control().other();
        self.emulator.set_link_control(next);
    }

    pub(super) fn link_control(&self) -> Option<Game> {
        self.emulator.link.as_ref().map(|link| link.control())
    }
}

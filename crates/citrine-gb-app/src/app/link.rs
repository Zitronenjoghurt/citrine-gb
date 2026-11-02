use crate::app::Citrine;
use crate::app::mobile::Sheet;
use crate::app::tabs::Tab;
use crate::app::ui_state::settings::SettingsTab;
use crate::local_link::Game;
use crate::partner_link::PartnerLink;
use crate::utils::file_loader::PickedFile;
use citrine_gb::rom::Rom;

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
        } else if self.emulator.open_game().is_none() {
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
        if let Some(path) = file.path.as_deref()
            && let Ok(header) = Rom::new(&file.data).header()
        {
            let name = crate::game_name::game_name(
                &header.sha1,
                Some(&file.name),
                &header.title,
                header.cgb_mode.is_cgb(),
            );
            self.ui.recent.record(path, &name, None);
        }
        self.link_game(file.data, Some(file.name));
    }

    pub(super) fn start_partner(&mut self, event: &'static citrine_gb_partner::Event) {
        let rom = self.emulator.gb.cartridge.rom_bytes();
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
        if self.mobile.sheet.is_some() {
            self.close_mobile_sheet();
        } else {
            self.open_tab(Tab::Playing);
        }
    }

    pub(super) fn link_game(&mut self, data: Vec<u8>, name: Option<String>) {
        self.try_start_audio();
        let rom = Rom::new(&data);
        let Ok(header) = rom.header() else {
            self.toasts.error("That file isn't a Game Boy game");
            return;
        };
        let model = self.ui.settings.console_for_rom(&rom);
        let title = crate::game_name::game_name(
            &header.sha1,
            name.as_deref(),
            &header.title,
            header.cgb_mode.is_cgb(),
        );
        if let Err(err) = self.emulator.start_link(&rom, model, title) {
            self.toasts
                .error(format!("Couldn't start the second game: {err}"));
            return;
        }
        self.ui.linked.picking = false;
        self.ui.settings.dirty = true;
        if let Some(link) = &mut self.emulator.link {
            link.set_settings(&self.ui.settings.local_link);
        }
        if self.mobile.sheet.is_some() {
            self.close_mobile_sheet();
        } else {
            self.open_tab(Tab::Playing);
        }
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
        if self.mobile.sheet == Some(Sheet::Panel(Tab::Playing)) {
            self.close_mobile_sheet();
        }
    }

    pub(super) fn open_settings(&mut self, tab: SettingsTab, touch: bool) {
        self.ui.settings.current_tab = tab;
        self.show_tab(Tab::Settings, touch);
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

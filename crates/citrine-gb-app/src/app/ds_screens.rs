use super::Citrine;
use crate::app::tabs::Tab;
use crate::app::ui_state::ds_screens::ScreensAction;
use crate::app::ui_state::settings::SettingsTab;
use crate::app::widgets::screens_menu;
use egui_dock::SurfaceIndex;

impl Citrine {
    pub(super) fn screens_action(&mut self, action: ScreensAction) {
        if !self.emulator.two_screens() {
            return;
        }
        let context = self.emulator.screen_context;
        self.ui.settings.ds_screens.apply(context, action);
        self.emulator.ds_screens = self.ui.settings.ds_screens;
        self.ui.settings.dirty = true;
    }

    pub(super) fn screens_chip(&self) -> Option<String> {
        self.emulator.two_screens().then(|| {
            let placement = self
                .ui
                .settings
                .ds_screens
                .placement(self.emulator.screen_context)
                .with_away(self.emulator.lent_screen());
            screens_menu::chip_label(placement, self.emulator.screen_tabs())
        })
    }

    pub(super) fn open_screen_settings(&mut self, phone: bool) {
        if phone {
            self.mobile.nav.open_settings(SettingsTab::Style);
            return;
        }
        self.ui.settings.current_tab = SettingsTab::Style;
        self.open_tab(Tab::Settings);
    }

    pub(super) fn sync_screen_tabs(&mut self) {
        let game = self.dock.find_tab(&Tab::GameBoy);
        let wanted = self.emulator.screen_tabs() && game.is_some();
        match (wanted, self.dock.find_tab(&Tab::BottomScreen)) {
            (true, None) => {
                let Some((surface, node, _)) = game else {
                    return;
                };
                if surface == SurfaceIndex::main() {
                    self.dock
                        .main_surface_mut()
                        .split_right(node, 0.5, vec![Tab::BottomScreen]);
                } else {
                    self.dock
                        .main_surface_mut()
                        .push_to_first_leaf(Tab::BottomScreen);
                }
            }
            (false, Some(at)) => {
                self.dock.remove_tab(at);
            }
            _ => {}
        }
    }
}

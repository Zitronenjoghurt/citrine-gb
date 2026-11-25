use super::Citrine;
use crate::app::tabs::Tab;
use crate::app::ui_state::ds_screens::ScreensAction;
use crate::app::ui_state::layout_picks::Place;
use crate::app::ui_state::screen_layouts::LayoutConsole;
use crate::app::ui_state::settings_tree::SettingsTab;
use egui_dock::SurfaceIndex;

impl Citrine {
    pub(super) fn screens_action(&mut self, action: ScreensAction) {
        if !self.emulator.two_screens() {
            return;
        }
        if action == ScreensAction::SeparateTabs {
            let context = self.emulator.screen_context;
            let settings = &mut self.ui.settings;
            settings.ds_screens.apply(context, action);
            self.emulator.ds_screens = settings
                .ds_screens
                .with_placements_of(&self.emulator.ds_screens);
            settings.dirty = true;
            return;
        }
        if self.editing_layout() {
            return;
        }
        let place = self.layout_place();
        self.ui
            .settings
            .act_on_layout(LayoutConsole::Ds, place, action);
        self.ui.layout_applied = None;
        self.show_layout_pick(place);
    }

    pub(super) fn screens_chip(&self) -> Option<String> {
        (self.emulator.has_game() && !self.emulator.linked()).then(|| {
            self.ui.layout_chip_label(
                LayoutConsole::of(self.emulator.played_kind()),
                self.ui.layout_applied_place().unwrap_or(Place::Window),
                self.emulator.screen_tabs(),
                self.emulator.lent_screen(),
            )
        })
    }

    pub(super) fn open_screen_settings(&mut self, phone: bool) {
        let tab = if self.emulator.two_screens() {
            SettingsTab::Ds
        } else {
            SettingsTab::Screen
        };
        if phone {
            self.mobile.nav.open_settings(tab);
            return;
        }
        self.ui.settings.current_tab = tab;
        self.ui.settings_list_open = false;
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

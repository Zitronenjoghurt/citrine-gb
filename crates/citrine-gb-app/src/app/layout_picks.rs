use super::Citrine;
use crate::app::ui_state::ds_screens::ScreenContext;
use crate::app::ui_state::layout_picks::{Pick, Place};
use crate::app::ui_state::screen_layouts::LayoutConsole;
use egui::Context;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LayoutApplied {
    console: LayoutConsole,
    place: Place,
    pick: Option<Pick>,
}

impl crate::app::ui_state::UiState {
    pub(crate) fn layout_applied_place(&self) -> Option<Place> {
        self.layout_applied.map(|applied| applied.place)
    }
}

impl Citrine {
    pub(super) fn follow_layout_pick(&mut self, ctx: &Context) {
        let place = if self.big_screen_on() {
            Place::BigScreen
        } else if self.ui.settings.layout.is_touch(ctx) {
            if super::mobile::landscape(ctx) {
                Place::Landscape
            } else {
                Place::Portrait
            }
        } else if self.ui.focus_mode || self.ui.settings.fullscreen {
            Place::FullScreen
        } else {
            Place::Window
        };
        if !self.editing_layout() {
            self.show_layout_pick(place);
        }
    }

    pub(super) fn layout_place(&self) -> Place {
        if let Some(applied) = self.ui.layout_applied {
            return applied.place;
        }
        if self.big_screen_on() {
            return Place::BigScreen;
        }
        match self.emulator.screen_context {
            ScreenContext::Desktop => Place::Window,
            ScreenContext::Portrait => Place::Portrait,
            ScreenContext::Landscape => Place::Landscape,
        }
    }

    pub(super) fn show_layout_pick(&mut self, place: Place) {
        let console = LayoutConsole::of(self.emulator.played_kind());
        let key = self
            .emulator
            .rom_key()
            .filter(|_| !self.emulator.shows_session());
        let resolved = self.ui.settings.game_layout_pick(key, console, place);
        if console == LayoutConsole::Ds {
            *self.emulator.ds_screens.placement_mut(place.context()) =
                self.ui.settings.pick_placement(resolved.pick, place);
        }
        let now = LayoutApplied {
            console,
            place,
            pick: resolved.chosen.then_some(resolved.pick),
        };
        if self.ui.layout_applied.replace(now) == Some(now) {
            return;
        }
        let view = place.view();
        let settings = &mut self.ui.settings;
        let spots = settings.pick_spots(console, place, resolved.pick);
        if settings.layouts.spots(console, view) != spots {
            settings.layouts.restore(console, view, spots);
            settings.dirty = true;
        }
    }
}

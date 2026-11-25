use super::{Pass, first_focus, focus_candidates, in_content};
use crate::app::mobile::nav::{Nav, Place};
use crate::app::tabs::Tab;
use crate::app::ui_state::settings_tree::SettingsTab;
use egui::{Context, Id};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Screen {
    place: Place,
    page: Option<Tab>,
    settings: Option<SettingsTab>,
    game: bool,
    menu: bool,
}

impl Screen {
    pub fn of(nav: &Nav, home: bool) -> Self {
        Self {
            place: nav.place(),
            page: nav.page(),
            settings: nav.settings_page(),
            game: nav.on_game(),
            menu: nav.more_open() || home,
        }
    }
}

#[derive(Default)]
pub struct Recall {
    seen: Vec<(Screen, Id)>,
    shown: Option<Screen>,
    drawing: Option<Screen>,
    pending: bool,
    stepped: bool,
}

impl Recall {
    pub fn drawing(&mut self, screen: Screen) {
        self.drawing = Some(screen);
    }

    pub fn stepped(&mut self) {
        self.stepped = true;
    }

    pub fn end(&mut self, ctx: &Context, screen: Screen, active: bool) -> Option<Id> {
        if self.shown.replace(screen) != Some(screen) {
            self.pending = true;
        }
        let drawn = self.drawing.take();
        if !active {
            self.pending = false;
            self.stepped = false;
            return None;
        }
        if drawn != Some(screen) {
            return None;
        }
        let live = focus_candidates(ctx, Pass::This);
        let rect = |id: Id| {
            live.iter()
                .find(|(live, _, _)| *live == id)
                .map(|(_, rect, _)| *rect)
        };
        let focused = ctx.memory(|m| m.focused()).filter(|id| rect(*id).is_some());
        let pending = std::mem::take(&mut self.pending);
        let stepped = std::mem::take(&mut self.stepped);
        let mut target = None;
        if pending && (focused.is_none() || stepped) {
            target = self
                .remembered(screen)
                .filter(|id| rect(*id).is_some())
                .or_else(|| stepped.then(|| first_focus(ctx, Pass::This)).flatten());
        }
        if let Some(id) = target.or(focused)
            && !screen.menu
            && rect(id).is_some_and(|rect| in_content(ctx, &rect))
        {
            self.seen.retain(|(seen, _)| *seen != screen);
            self.seen.push((screen, id));
        }
        target
    }

    fn remembered(&self, screen: Screen) -> Option<Id> {
        self.seen
            .iter()
            .find(|(seen, _)| *seen == screen)
            .map(|(_, id)| *id)
    }
}

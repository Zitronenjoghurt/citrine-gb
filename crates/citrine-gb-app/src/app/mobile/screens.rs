use crate::app::Citrine;
use crate::app::mobile::kit::{self, Row, SwitchRow};
use crate::app::mobile::sheet;
use crate::app::ui_state::ds_screens::{Placement, ScreensAction};
use crate::app::ui_state::hotkeys::HotkeyAction;
use crate::app::widgets::screens_menu::{self, Picked, TOUCH_NOTE};
use crate::icons;
use egui::{Context, Id, Ui};

impl Citrine {
    pub(super) fn screens_icon(&self) -> &'static str {
        if self.emulator.lid_closed() {
            return icons::MOON;
        }
        let screens = &self.ui.settings.ds_screens;
        screens
            .placement(self.emulator.screen_context)
            .with_away(self.emulator.lent_screen())
            .chip()
            .0
    }

    pub(super) fn screens_sheet(&mut self, ctx: &Context) {
        if !self.mobile.screens_sheet {
            return;
        }
        if !self.emulator.two_screens() || !self.mobile.nav.on_game() {
            self.mobile.screens_sheet = false;
            return;
        }
        let placement = self
            .ui
            .settings
            .ds_screens
            .placement(self.emulator.screen_context);
        let closed = self.emulator.lid();
        let shown = sheet::show(ctx, Id::new("screens_sheet"), "", |ui| {
            sheet_list(ui, placement, closed)
        });
        match shown.inner {
            Some(Choice::Screens(Picked::Act(action))) => self.screens_action(action),
            Some(Choice::Lid) => self.run_hotkey(HotkeyAction::ToggleLid),
            Some(Choice::Screens(Picked::More)) => {
                self.mobile.screens_sheet = false;
                self.open_screen_settings(true);
            }
            None => {}
        }
        if shown.closed {
            self.mobile.screens_sheet = false;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    Screens(Picked),
    Lid,
}

fn sheet_list(ui: &mut Ui, placement: Placement, lid: Option<bool>) -> Option<Choice> {
    let chosen = (!placement.one_screen).then_some(placement.arrangement);
    let mut picked = screens_menu::tiles(ui, chosen)
        .map(|arrangement| Choice::Screens(Picked::Act(ScreensAction::Arrange(arrangement))));
    kit::list(ui, |list| {
        let mut swapped = placement.swapped;
        if list.switch(SwitchRow::new("Swap screens", &mut swapped)) {
            picked = Some(Choice::Screens(Picked::Act(ScreensAction::Swap)));
        }
        let mut one = placement.one_screen;
        let note = placement.touch_hidden().then_some(TOUCH_NOTE);
        if list.switch(SwitchRow::new(placement.only_label(), &mut one).detail(note)) {
            picked = Some(Choice::Screens(Picked::Act(ScreensAction::OneScreen)));
        }
        if let Some(mut closed) = lid {
            let label = HotkeyAction::ToggleLid.label();
            if list.switch(SwitchRow::new(label, &mut closed)) {
                picked = Some(Choice::Lid);
            }
        }
        if list.row(Row::new("More screen settings")).clicked() {
            picked = Some(Choice::Screens(Picked::More));
        }
    });
    picked
}

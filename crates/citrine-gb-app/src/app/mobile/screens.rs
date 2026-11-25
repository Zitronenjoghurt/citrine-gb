use crate::app::Citrine;
use crate::app::big_screen::glyphs::Family;
use crate::app::layout_sheet::{SheetFacts, SheetPick};
use crate::app::mobile::kit::{self, CompactRow};
use crate::app::mobile::settings_pages::scope::{segments_of, tag_chip};
use crate::app::mobile::sheet;
use crate::app::ui_state::layout_picks::Place;
use crate::app::ui_state::per_game::Scope;
use crate::app::ui_state::screen_layouts::LayoutConsole;
use crate::app::widgets::screens_menu::{self, CardSize};
use crate::app::widgets::settings::scope;
use crate::icons;
use egui::{Align, Context, Id, Layout, RichText, Ui};

impl Citrine {
    pub(super) fn screens_icon(&self) -> &'static str {
        if self.emulator.lid_closed() {
            return icons::MOON;
        }
        let placement = self
            .emulator
            .ds_screens
            .placement(self.emulator.screen_context)
            .with_away(self.emulator.lent_screen());
        screens_menu::chip_icon(LayoutConsole::of(self.emulator.played_kind()), placement)
    }

    pub(super) fn side_sheet_open(&self) -> bool {
        self.ui.layout_sheet.is_some()
            && !self.big_screen_on()
            && self.emulator.has_game()
            && !self.editing_layout()
    }

    pub(super) fn phone_layout_sheet(&mut self, ctx: &Context) {
        if self.ui.layout_sheet.is_none() || self.big_screen_on() {
            return;
        }
        if !self.emulator.has_game() || !self.mobile.nav.on_game() || self.editing_layout() {
            self.ui.layout_sheet = None;
            return;
        }
        let device = self.layout_device(ctx);
        let facts = self.ui.layout_facts(&device);
        let sheet_state = self.ui.layout_sheet.as_ref();
        let menu = sheet_state.and_then(|sheet| sheet.menu);
        let layer = sheet_state.is_some_and(|sheet| sheet.layer_open());
        let id = Id::new("layout_sheet");
        let body = |ui: &mut Ui| sheet_body(ui, &facts, menu, false, None, false);
        let shown = if device.here == Place::Landscape {
            sheet::side(ctx, id, body)
        } else {
            sheet::show(ctx, id, "", body)
        };
        if let Some(pick) = shown.inner
            && let Some(go) = self.ui.pick_in_sheet(&device, pick)
        {
            self.sheet_go(go, true);
        }
        if shown.outside {
            self.ui.layout_sheet = None;
        } else if shown.closed {
            self.close_layout_sheet_layer(layer);
        }
    }

    pub(in crate::app) fn close_layout_sheet_layer(&mut self, layer: bool) {
        let Some(sheet) = &mut self.ui.layout_sheet else {
            return;
        };
        if layer {
            sheet.close_layer();
        } else if sheet.other.is_some() {
            sheet.other = None;
        } else {
            self.ui.layout_sheet = None;
        }
    }
}

pub(in crate::app) fn sheet_body(
    ui: &mut Ui,
    facts: &SheetFacts,
    menu: Option<u32>,
    big: bool,
    shoulders: Option<Family>,
    focus: bool,
) -> Option<SheetPick> {
    let mut picked = None;
    sheet::inset(ui, |ui| {
        ui.horizontal(|ui| {
            if !facts.here
                && kit::labeled(
                    ui.add(kit::header_button(
                        icons::ARROW_LEFT,
                        ui.spacing().interact_size.y,
                    )),
                    "Back",
                )
                .on_hover_text("Back")
                .clicked()
            {
                picked = Some(SheetPick::Back);
            }
            let size = if big { 22.0 } else { 19.0 };
            ui.label(RichText::new(screens_menu::TITLE).size(size).strong());
            if let (Some(tag), Some(game)) = (&facts.tag, &facts.game) {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if tag_chip(ui, tag, game, big) {
                        picked = Some(SheetPick::DropGame);
                    }
                });
            }
        });
    });
    ui.add_space(10.0);
    if let Some(game) = &facts.game {
        let mut games = facts.level.games;
        let options = [
            (Scope::AllGames, scope::ALL_GAMES.to_owned()),
            (Scope::ThisGame, format!("{} {game}", scope::MARK)),
        ];
        segments_of(ui, Id::new("layout_games"), &mut games, &options, shoulders);
        if games != facts.level.games {
            picked = Some(SheetPick::Games(games));
        }
        ui.add_space(8.0);
    }
    let mut every = facts.level.every_mode;
    let options = [
        (true, screens_menu::EVERY_MODE.to_owned()),
        (false, facts.mode.to_owned()),
    ];
    segments_of(ui, Id::new("layout_modes"), &mut every, &options, None);
    if every != facts.level.every_mode {
        picked = Some(SheetPick::Modes(every));
    }
    ui.add_space(12.0);
    let pressed = if big {
        let columns = screens_menu::grid_columns(facts.cards.len());
        let size = CardSize::grid(ui.available_width(), 15.0, columns);
        screens_menu::card_grid(ui, &facts.cards, size, menu, focus)
    } else {
        screens_menu::card_row(ui, &facts.cards, CardSize::row(facts.place), menu)
    };
    if let Some(press) = pressed {
        picked = Some(screens_menu::card_pick(press));
    }
    ui.add_space(8.0);
    kit::groups(ui, !big, |groups| {
        let edits = facts.here || facts.turn;
        if edits || facts.swapped.is_some() {
            groups.group(|ui| {
                let new = CompactRow::new(icons::PLUS, screens_menu::NEW_LAYOUT);
                if edits && new.show(ui).clicked() {
                    picked = Some(SheetPick::New);
                }
                let edit = CompactRow::new(icons::PENCIL_SIMPLE, screens_menu::EDIT);
                if edits && edit.show(ui).clicked() {
                    picked = Some(SheetPick::Edit);
                }
                if let Some(swapped) = facts.swapped
                    && CompactRow::new(screens_menu::SWAP_ICON, screens_menu::SWAP)
                        .switch(swapped)
                        .show(ui)
                        .clicked()
                {
                    picked = Some(SheetPick::Swap);
                }
            });
        }
        if !facts.others.is_empty() {
            groups.group(|ui| {
                for (place, name) in &facts.others {
                    let row = CompactRow::new(place.icon(), place.row_label())
                        .turned(place.icon_turned())
                        .value(name.clone());
                    if row.show(ui).clicked() {
                        picked = Some(SheetPick::Place(*place));
                    }
                }
            });
        }
        groups.group(|ui| {
            if let Some(closed) = facts.lid
                && CompactRow::new(icons::LAPTOP, screens_menu::LID)
                    .switch(closed)
                    .show(ui)
                    .clicked()
            {
                picked = Some(SheetPick::Lid);
            }
            let more = CompactRow::new(icons::SLIDERS_HORIZONTAL, screens_menu::MORE).chevron();
            if more.show(ui).clicked() {
                picked = Some(SheetPick::More);
            }
        });
    });
    picked
}

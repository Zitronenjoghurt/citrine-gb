use crate::app::Citrine;
use crate::app::layout_edit::EditStart;
use crate::app::ui_state::UiState;
use crate::app::ui_state::ds_screens::{Arrangement, Placement, ScreensAction};
use crate::app::ui_state::hotkeys::HotkeyAction;
use crate::app::ui_state::layout_picks::{Builtin, Pick, Place, Preset, Resolved, STANDARD};
use crate::app::ui_state::per_game::Scope;
use crate::app::ui_state::screen_layouts::{LayoutConsole, Spots};
use crate::app::widgets::screens_menu::{Card, CardKind, Keys, Picture};
use crate::app::widgets::settings::scope::{self, Tag};
use crate::app::widgets::{confirm, screens_menu};
use crate::emulator::Emulator;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Level {
    pub games: Scope,
    pub every_mode: bool,
}

impl Level {
    pub fn of(resolved: &Resolved) -> Self {
        Self {
            games: if resolved.game {
                Scope::ThisGame
            } else {
                Scope::AllGames
            },
            every_mode: resolved.every_mode,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct LayoutSheet {
    pub other: Option<Place>,
    levels: Vec<(Place, Level)>,
    pub menu: Option<u32>,
    pub rename: Option<(u32, String)>,
    pub delete: Option<u32>,
    focus_placed: bool,
}

impl LayoutSheet {
    pub fn take_first_focus(&mut self) -> bool {
        !std::mem::replace(&mut self.focus_placed, true)
    }

    pub fn layer_open(&self) -> bool {
        self.menu.is_some() || self.rename.is_some() || self.delete.is_some()
    }

    pub fn close_layer(&mut self) {
        self.menu = None;
        self.rename = None;
        self.delete = None;
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Device {
    pub console: LayoutConsole,
    pub here: Place,
    pub places: Vec<Place>,
    pub lid: Option<bool>,
    pub tabs: Option<bool>,
    pub keys: Keys,
}

#[derive(Debug, Clone)]
pub(crate) struct SheetFacts {
    pub place: Place,
    pub here: bool,
    pub game: Option<String>,
    pub mode: &'static str,
    pub level: Level,
    pub tag: Option<Tag>,
    pub cards: Vec<Card>,
    pub swapped: Option<bool>,
    pub others: Vec<(Place, String)>,
    pub turn: bool,
    pub lid: Option<bool>,
    pub tabs: Option<bool>,
    pub keys: Keys,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SheetPick {
    Games(Scope),
    Modes(bool),
    DropGame,
    Card(CardKind),
    New,
    Menu(u32),
    CardMenu(u32, CardMenuPick),
    CloseMenu,
    Edit,
    Swap,
    Place(Place),
    Back,
    Lid,
    SeparateTabs,
    More,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SheetGo {
    Edit(EditStart),
    Turn(Place, EditStart),
    Lid,
    SeparateTabs,
    More,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LayoutTurn {
    pub place: Place,
    pub start: EditStart,
    pub notice: Option<&'static str>,
}

pub(crate) fn turn_notice(place: Place, new: bool) -> &'static str {
    match (place, new) {
        (Place::Landscape, true) => "Turn your phone to make a landscape layout",
        (Place::Landscape, false) => "Turn your phone to edit the landscape layout",
        (_, true) => "Turn your phone to make a portrait layout",
        (_, false) => "Turn your phone to edit the portrait layout",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CardMenuPick {
    Rename,
    Delete,
    Reset,
}

pub(crate) fn device_places(here: Place, phone: bool) -> Vec<Place> {
    match here {
        Place::Portrait | Place::Landscape => {
            vec![Place::Portrait, Place::Landscape, Place::BigScreen]
        }
        Place::Window | Place::FullScreen => {
            vec![Place::Window, Place::FullScreen, Place::BigScreen]
        }
        Place::BigScreen if crate::platform::is_tv() => vec![Place::BigScreen],
        Place::BigScreen if phone => vec![Place::Portrait, Place::Landscape, Place::BigScreen],
        Place::BigScreen => vec![Place::Window, Place::FullScreen, Place::BigScreen],
    }
}

fn shown_placement(ui: &UiState, resolved: &Resolved, place: Place) -> Placement {
    ui.settings.pick_placement(resolved.pick, place)
}

fn swap_shows(pick: Pick) -> bool {
    match pick {
        Pick::Preset(placement) => {
            !placement.one_screen && placement.arrangement != Arrangement::BigAndSmall
        }
        Pick::Standard | Pick::Saved(_) => true,
    }
}

fn preset_pick(preset: Preset, now: Placement) -> Pick {
    let mut placement = preset.placement();
    let ordered = matches!(
        placement.arrangement,
        Arrangement::Stacked | Arrangement::SideBySide
    );
    if ordered && !placement.one_screen && !now.one_screen {
        placement.swapped = now.swapped;
    }
    Pick::Preset(placement)
}

impl UiState {
    pub(crate) fn layout_chip_label(
        &self,
        console: LayoutConsole,
        place: Place,
        tabs: bool,
        away: Option<usize>,
    ) -> String {
        let shown = self.settings.shown_layout(console, place);
        let placement = shown.placement.with_away(away);
        let name = match away {
            Some(_) => placement.chip().1.to_owned(),
            None => self.settings.pick_name(shown.resolved.pick),
        };
        screens_menu::chip_label(&name, screens_menu::chip_icon(console, placement), tabs)
    }

    pub(crate) fn sheet_place(&self, device: &Device) -> Place {
        self.layout_sheet
            .as_ref()
            .and_then(|sheet| sheet.other)
            .unwrap_or(device.here)
    }

    pub(crate) fn sheet_level(&mut self, device: &Device) -> Level {
        let place = self.sheet_place(device);
        let resolved = self.settings.layout_pick(device.console, place);
        let sheet = self.layout_sheet.get_or_insert_with(LayoutSheet::default);
        if let Some((_, level)) = sheet.levels.iter().find(|(at, _)| *at == place) {
            return *level;
        }
        let level = Level::of(&resolved);
        sheet.levels.push((place, level));
        level
    }

    fn set_sheet_level(&mut self, device: &Device, change: impl FnOnce(&mut Level)) {
        let place = self.sheet_place(device);
        let mut level = self.sheet_level(device);
        change(&mut level);
        if let Some(sheet) = &mut self.layout_sheet {
            sheet.levels.retain(|(at, _)| *at != place);
            sheet.levels.push((place, level));
        }
    }

    fn sheet_resolved(&mut self, device: &Device) -> (Place, Level, Resolved) {
        let place = self.sheet_place(device);
        let level = self.sheet_level(device);
        let resolved =
            self.settings
                .layout_pick_from(device.console, place, level.games, level.every_mode);
        (place, level, resolved)
    }

    pub(crate) fn layout_facts(&mut self, device: &Device) -> SheetFacts {
        let (place, level, resolved) = self.sheet_resolved(device);
        let placement = shown_placement(self, &resolved, place);
        let console = device.console;
        let settings = &self.settings;
        let here = place == device.here;
        let mut cards = Vec::new();
        let picture = |placement: Placement, spots: Spots| {
            Some(Picture {
                console,
                place,
                placement,
                spots,
            })
        };
        let edit = |builtin: Builtin| {
            settings
                .builtin_edit(console, place, builtin)
                .map(|edit| edit.id)
        };
        if console == LayoutConsole::Ds {
            let mut presets = place.presets().to_vec();
            if let Pick::Preset(chosen) = resolved.pick
                && !presets.contains(&Preset::of(chosen))
            {
                presets.push(Preset::of(chosen));
            }
            for preset in presets {
                let chosen = matches!(resolved.pick, Pick::Preset(p) if Preset::of(p) == preset);
                let drawn = if chosen {
                    placement
                } else {
                    preset.placement()
                };
                let key = (preset == Preset::TopOnly)
                    .then(|| device.keys.one.clone())
                    .flatten();
                let spots = settings.pick_spots(console, place, Pick::Preset(drawn));
                cards.push(Card {
                    kind: CardKind::Preset(preset),
                    name: preset.label().to_owned(),
                    chosen,
                    picture: picture(drawn, spots),
                    key,
                    edit: edit(Builtin::Preset(preset)),
                });
            }
        } else {
            cards.push(Card {
                kind: CardKind::Standard,
                name: STANDARD.to_owned(),
                chosen: resolved.pick == Pick::Standard,
                picture: picture(
                    Placement::default(),
                    settings.pick_spots(console, place, Pick::Standard),
                ),
                key: None,
                edit: edit(Builtin::Standard),
            });
        }
        let mut saved: Vec<_> = settings.saved_layouts_in(console, place).collect();
        if let Pick::Saved(id) = resolved.pick
            && !saved.iter().any(|s| s.id == id)
            && let Some(extra) = settings.saved_layout(id)
        {
            saved.push(extra);
        }
        for layout in saved {
            cards.push(Card {
                kind: CardKind::Saved(layout.id),
                name: layout.name.clone(),
                chosen: resolved.pick == Pick::Saved(layout.id),
                picture: picture(layout.placement, layout.spots.clone()),
                key: None,
                edit: None,
            });
        }
        let ds = console == LayoutConsole::Ds;
        let others = device
            .places
            .iter()
            .filter(|other| **other != place)
            .map(|other| {
                let pick = settings.layout_pick(console, *other).pick;
                (*other, settings.pick_name(pick))
            })
            .collect();
        let game_pick = |resolved: Resolved| Tag::Pick {
            game: scope::game_title(settings).to_owned(),
            value: settings.pick_name(resolved.pick),
        };
        let tag = settings.playing_key().and_then(|_| match level.games {
            Scope::ThisGame => {
                let own =
                    settings.layout_pick_from(console, place, Scope::ThisGame, level.every_mode);
                Some(if own.game {
                    game_pick(own)
                } else {
                    Tag::Follows
                })
            }
            Scope::AllGames => {
                let wins = settings.layout_pick(console, place);
                wins.game.then(|| game_pick(wins))
            }
        });
        SheetFacts {
            place,
            here,
            game: settings
                .playing_key()
                .map(|_| scope::game_title(settings).to_owned()),
            mode: place.title(),
            level,
            tag,
            cards,
            swapped: (ds && swap_shows(resolved.pick)).then_some(placement.swapped),
            others,
            turn: !here && place.turns_from(device.here),
            lid: device.lid.filter(|_| ds),
            tabs: device.tabs.filter(|_| ds),
            keys: device.keys.clone(),
        }
    }

    pub(crate) fn pick_in_sheet(&mut self, device: &Device, pick: SheetPick) -> Option<SheetGo> {
        let (place, level, resolved) = self.sheet_resolved(device);
        let console = device.console;
        match pick {
            SheetPick::Games(games) => self.set_sheet_level(device, |level| level.games = games),
            SheetPick::Modes(every_mode) => {
                self.set_sheet_level(device, |level| level.every_mode = every_mode);
            }
            SheetPick::DropGame => {
                let own = self.settings.layout_pick_from(
                    console,
                    place,
                    Scope::ThisGame,
                    level.every_mode,
                );
                if own.game {
                    self.settings.clear_layout_pick(
                        console,
                        place,
                        Scope::ThisGame,
                        own.every_mode,
                    );
                }
            }
            SheetPick::New | SheetPick::Edit => {
                self.layout_sheet = None;
                let start = EditStart {
                    new: pick == SheetPick::New,
                    level: Some(level),
                };
                return Some(if place == device.here {
                    SheetGo::Edit(start)
                } else {
                    SheetGo::Turn(place, start)
                });
            }
            SheetPick::Card(kind) => {
                let now = shown_placement(self, &resolved, place);
                let next = match kind {
                    CardKind::Preset(preset) => preset_pick(preset, now),
                    CardKind::Saved(id) => Pick::Saved(id),
                    CardKind::Standard => Pick::Standard,
                };
                self.settings
                    .set_layout_pick(console, place, next, level.games, level.every_mode);
            }
            SheetPick::Menu(id) => {
                if let Some(sheet) = &mut self.layout_sheet {
                    sheet.menu = Some(id);
                }
            }
            SheetPick::CardMenu(id, pick) => self.card_menu_pick(id, pick),
            SheetPick::CloseMenu => {
                if let Some(sheet) = &mut self.layout_sheet {
                    sheet.menu = None;
                }
            }
            SheetPick::Swap => {
                self.settings
                    .swap_layout_at(console, place, level.games, level.every_mode);
                self.layout_applied = None;
            }
            SheetPick::Place(other) => {
                if let Some(sheet) = &mut self.layout_sheet {
                    sheet.other = (other != device.here).then_some(other);
                }
            }
            SheetPick::Back => {
                if let Some(sheet) = &mut self.layout_sheet {
                    sheet.other = None;
                }
            }
            SheetPick::Lid => return Some(SheetGo::Lid),
            SheetPick::SeparateTabs => return Some(SheetGo::SeparateTabs),
            SheetPick::More => {
                self.layout_sheet = None;
                return Some(SheetGo::More);
            }
        }
        None
    }

    pub(crate) fn card_menu_pick(&mut self, id: u32, pick: CardMenuPick) {
        let Some(sheet) = &mut self.layout_sheet else {
            return;
        };
        sheet.menu = None;
        match pick {
            CardMenuPick::Rename => {
                let name = self
                    .settings
                    .saved_layout(id)
                    .map(|saved| saved.name.clone())
                    .unwrap_or_default();
                sheet.rename = Some((id, name));
            }
            CardMenuPick::Delete | CardMenuPick::Reset => sheet.delete = Some(id),
        }
    }

    pub(crate) fn layout_sheet_dialogs(&mut self, ctx: &egui::Context) {
        let Some(sheet) = &mut self.layout_sheet else {
            return;
        };
        if let Some((id, name)) = &mut sheet.rename {
            let id = *id;
            match confirm::name(ctx, "rename_layout", "Rename layout", name, "Rename") {
                Some(true) => {
                    let name = name.clone();
                    sheet.rename = None;
                    self.settings.rename_layout(id, &name);
                }
                Some(false) => sheet.rename = None,
                None => {}
            }
            return;
        }
        if let Some(id) = sheet.delete {
            let (name, builtin) = self
                .settings
                .saved_layout(id)
                .map(|saved| (saved.name.clone(), saved.builtin.is_some()))
                .unwrap_or_default();
            let (question, button) = if builtin {
                (reset_question(&name), screens_menu::RESET)
            } else {
                (format!("Delete {name}?"), screens_menu::DELETE)
            };
            match confirm::confirm(ctx, "delete_layout", &question, button) {
                Some(true) => {
                    sheet.delete = None;
                    if builtin {
                        self.settings.reset_builtin(id);
                    } else {
                        self.settings.delete_layout(id);
                    }
                    self.layout_applied = None;
                }
                Some(false) => sheet.delete = None,
                None => {}
            }
        }
    }
}

pub(crate) fn reset_question(name: &str) -> String {
    format!("Reset {name}? This undoes your changes to it.")
}

pub(crate) fn device(
    ui: &UiState,
    emulator: &Emulator,
    here: Place,
    phone: bool,
    desktop: bool,
) -> Device {
    let hotkeys = &ui.settings.hotkeys;
    let key = |action: HotkeyAction| desktop.then(|| hotkeys.key_name(action)).flatten();
    Device {
        console: LayoutConsole::of(emulator.played_kind()),
        here,
        places: device_places(here, phone),
        lid: emulator.lid(),
        tabs: desktop.then(|| emulator.screen_tabs()),
        keys: Keys {
            swap: key(HotkeyAction::SwapScreens),
            one: key(HotkeyAction::OneScreen),
        },
    }
}

impl Citrine {
    pub(crate) fn layout_device(&self, ctx: &egui::Context) -> Device {
        let phone = self.ui.settings.layout.is_touch(ctx);
        let desktop = !self.is_mobile(ctx);
        device(
            &self.ui,
            &self.emulator,
            self.layout_place(),
            phone,
            desktop,
        )
    }

    pub(crate) fn sheet_go(&mut self, go: SheetGo, phone: bool) {
        match go {
            SheetGo::Edit(start) => self.start_layout_edit_with(start),
            SheetGo::Turn(place, start) => self.turn_to_edit(place, start),
            SheetGo::Lid => self.run_hotkey(HotkeyAction::ToggleLid),
            SheetGo::SeparateTabs => self.screens_action(ScreensAction::SeparateTabs),
            SheetGo::More => self.open_screen_settings(phone),
        }
    }

    fn turn_to_edit(&mut self, place: Place, start: EditStart) {
        let held = crate::platform::hold_orientation(Some(place == Place::Landscape));
        self.ui.orientation_held |= held;
        let notice = (!held).then(|| turn_notice(place, start.new));
        if let Some(text) = notice {
            self.toasts.info(text).duration(None).needs_action();
        }
        self.ui.layout_turn = Some(LayoutTurn {
            place,
            start,
            notice,
        });
    }

    pub(crate) fn layout_turn_wait(&mut self, on_game: bool) {
        let Some(turn) = self.ui.layout_turn else {
            if self.ui.orientation_held && !self.editing_layout() {
                self.ui.orientation_held = false;
                crate::platform::hold_orientation(None);
            }
            return;
        };
        let dismissed = turn.notice.is_some_and(|text| !self.toasts.is_open(text));
        let turned = self.layout_place() == turn.place;
        if turned || dismissed || !on_game || !self.emulator.has_game() || self.big_screen_on() {
            self.ui.layout_turn = None;
            if let Some(text) = turn.notice {
                self.toasts.close(text);
            }
        }
        if turned && on_game && self.emulator.has_game() && !self.big_screen_on() {
            self.start_layout_edit_with(turn.start);
        }
    }
}

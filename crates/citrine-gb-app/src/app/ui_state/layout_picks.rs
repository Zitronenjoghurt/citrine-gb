use crate::app::ui_state::ds_screens::{
    Arrangement, DsScreens, Placement, ScreenContext, ScreensAction,
};
use crate::app::ui_state::per_game::Scope;
use crate::app::ui_state::screen_layouts::{Item, LayoutConsole, LayoutView, Spots};
use crate::app::ui_state::settings::Settings;
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

pub const MIGRATED_NAME: &str = "My layout";
pub const STANDARD: &str = "Standard";

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    EnumIter,
)]
pub enum Place {
    Portrait,
    Landscape,
    Window,
    FullScreen,
    BigScreen,
}

impl Place {
    pub fn view(self) -> LayoutView {
        match self {
            Self::Portrait => LayoutView::Portrait,
            Self::Landscape => LayoutView::Landscape,
            Self::Window | Self::FullScreen => LayoutView::Desktop,
            Self::BigScreen => LayoutView::BigScreen,
        }
    }

    pub fn context(self) -> ScreenContext {
        match self {
            Self::Portrait => ScreenContext::Portrait,
            Self::Landscape | Self::BigScreen => ScreenContext::Landscape,
            Self::Window | Self::FullScreen => ScreenContext::Desktop,
        }
    }

    pub fn of_view(view: LayoutView) -> Self {
        match view {
            LayoutView::Portrait => Self::Portrait,
            LayoutView::Landscape => Self::Landscape,
            LayoutView::Desktop => Self::Window,
            LayoutView::BigScreen => Self::BigScreen,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Portrait => "Portrait",
            Self::Landscape => "Landscape",
            Self::Window => "Window",
            Self::FullScreen => "Full screen",
            Self::BigScreen => "Big screen",
        }
    }

    pub fn row_label(self) -> &'static str {
        match self {
            Self::Portrait => "In portrait",
            Self::Landscape => "In landscape",
            Self::Window => "In a window",
            Self::FullScreen => "In full screen",
            Self::BigScreen => "On the big screen",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Portrait => crate::icons::DEVICE_MOBILE,
            Self::Landscape => crate::icons::DEVICE_MOBILE,
            Self::Window => crate::icons::APP_WINDOW,
            Self::FullScreen => crate::icons::CORNERS_OUT,
            Self::BigScreen => crate::icons::TELEVISION_SIMPLE,
        }
    }

    pub fn turns_from(self, other: Place) -> bool {
        matches!(
            (self, other),
            (Self::Portrait, Self::Landscape) | (Self::Landscape, Self::Portrait)
        )
    }

    pub fn icon_turned(self) -> bool {
        self == Self::Landscape
    }

    pub fn presets(self) -> &'static [Preset] {
        match self {
            Self::Portrait => &[Preset::Stacked, Preset::TopOnly, Preset::BottomOnly],
            Self::Landscape | Self::Window | Self::FullScreen | Self::BigScreen => &[
                Preset::Stacked,
                Preset::SideBySide,
                Preset::BigTop,
                Preset::BigBottom,
                Preset::TopOnly,
                Preset::BottomOnly,
            ],
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Portrait => "portrait",
            Self::Landscape => "landscape",
            Self::Window => "window",
            Self::FullScreen => "full_screen",
            Self::BigScreen => "big_screen",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter)]
pub enum Preset {
    Stacked,
    SideBySide,
    BigTop,
    BigBottom,
    TopOnly,
    BottomOnly,
}

impl Preset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stacked => "Stacked",
            Self::SideBySide => "Side by side",
            Self::BigTop => "Big top",
            Self::BigBottom => "Big bottom",
            Self::TopOnly => "Top only",
            Self::BottomOnly => "Bottom only",
        }
    }

    pub fn placement(self) -> Placement {
        let (arrangement, swapped, one_screen) = match self {
            Self::Stacked => (Arrangement::Stacked, false, false),
            Self::SideBySide => (Arrangement::SideBySide, false, false),
            Self::BigTop => (Arrangement::BigAndSmall, false, false),
            Self::BigBottom => (Arrangement::BigAndSmall, true, false),
            Self::TopOnly => (Arrangement::Stacked, false, true),
            Self::BottomOnly => (Arrangement::Stacked, true, true),
        };
        Placement {
            arrangement,
            swapped,
            one_screen,
        }
    }

    pub fn of(placement: Placement) -> Self {
        match (
            placement.one_screen,
            placement.arrangement,
            placement.swapped,
        ) {
            (true, _, false) => Self::TopOnly,
            (true, _, true) => Self::BottomOnly,
            (false, Arrangement::Stacked, _) => Self::Stacked,
            (false, Arrangement::SideBySide, _) => Self::SideBySide,
            (false, Arrangement::BigAndSmall, false) => Self::BigTop,
            (false, Arrangement::BigAndSmall, true) => Self::BigBottom,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Pick {
    Preset(Placement),
    Standard,
    Saved(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Builtin {
    Preset(Preset),
    Standard,
}

impl Builtin {
    pub fn of(pick: Pick) -> Option<Self> {
        match pick {
            Pick::Preset(placement) => Some(Self::Preset(Preset::of(placement))),
            Pick::Standard => Some(Self::Standard),
            Pick::Saved(_) => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Preset(preset) => preset.label(),
            Self::Standard => STANDARD,
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SavedLayout {
    pub id: u32,
    pub name: String,
    pub console: LayoutConsole,
    pub place: Place,
    pub placement: Placement,
    pub spots: Spots,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builtin: Option<Builtin>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ConsolePicks {
    #[serde(deserialize_with = "known_pick")]
    pub every_mode: Option<Pick>,
    #[serde(deserialize_with = "known_pick")]
    pub portrait: Option<Pick>,
    #[serde(deserialize_with = "known_pick")]
    pub landscape: Option<Pick>,
    #[serde(deserialize_with = "known_pick")]
    pub window: Option<Pick>,
    #[serde(deserialize_with = "known_pick")]
    pub full_screen: Option<Pick>,
    #[serde(deserialize_with = "known_pick")]
    pub big_screen: Option<Pick>,
}

fn known_pick<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<Pick>, D::Error> {
    let value = <serde_json::Value as serde::Deserialize>::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).ok().flatten())
}

impl ConsolePicks {
    pub fn get(&self, at: Option<Place>) -> Option<Pick> {
        match at {
            None => self.every_mode,
            Some(Place::Portrait) => self.portrait,
            Some(Place::Landscape) => self.landscape,
            Some(Place::Window) => self.window,
            Some(Place::FullScreen) => self.full_screen,
            Some(Place::BigScreen) => self.big_screen,
        }
    }

    fn slot(&mut self, at: Option<Place>) -> &mut Option<Pick> {
        match at {
            None => &mut self.every_mode,
            Some(Place::Portrait) => &mut self.portrait,
            Some(Place::Landscape) => &mut self.landscape,
            Some(Place::Window) => &mut self.window,
            Some(Place::FullScreen) => &mut self.full_screen,
            Some(Place::BigScreen) => &mut self.big_screen,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LayoutPicks {
    pub game_boy: ConsolePicks,
    pub gba: ConsolePicks,
    pub ds: ConsolePicks,
}

impl LayoutPicks {
    pub fn of(&self, console: LayoutConsole) -> &ConsolePicks {
        match console {
            LayoutConsole::GameBoy => &self.game_boy,
            LayoutConsole::Gba => &self.gba,
            LayoutConsole::Ds => &self.ds,
        }
    }

    fn of_mut(&mut self, console: LayoutConsole) -> &mut ConsolePicks {
        match console {
            LayoutConsole::GameBoy => &mut self.game_boy,
            LayoutConsole::Gba => &mut self.gba,
            LayoutConsole::Ds => &mut self.ds,
        }
    }
}

pub fn pick_path(console: LayoutConsole, at: Option<Place>) -> String {
    let console = match console {
        LayoutConsole::GameBoy => "game_boy",
        LayoutConsole::Gba => "gba",
        LayoutConsole::Ds => "ds",
    };
    format!(
        "layout_picks.{console}.{}",
        at.map_or("every_mode", Place::key)
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolved {
    pub pick: Pick,
    pub game: bool,
    pub every_mode: bool,
    pub chosen: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShownLayout {
    pub resolved: Resolved,
    pub placement: Placement,
    pub spots: Spots,
}

impl Settings {
    fn usable(&self, console: LayoutConsole, pick: Pick) -> bool {
        match pick {
            Pick::Preset(_) => console == LayoutConsole::Ds,
            Pick::Standard => console != LayoutConsole::Ds,
            Pick::Saved(id) => self.saved_layout(id).is_some_and(|s| s.console == console),
        }
    }

    fn game_pick(
        &self,
        key: Option<&str>,
        console: LayoutConsole,
        at: Option<Place>,
    ) -> Option<Pick> {
        let own = self.own_value(key?, &pick_path(console, at))?;
        serde_json::from_value::<Option<Pick>>(own.clone())
            .ok()
            .flatten()
    }

    fn default_pick(&self, console: LayoutConsole, place: Place) -> Pick {
        match console {
            LayoutConsole::Ds => Pick::Preset(self.ds_default(place)),
            LayoutConsole::GameBoy | LayoutConsole::Gba => Pick::Standard,
        }
    }

    fn ds_default(&self, place: Place) -> Placement {
        let placement = self.ds_screens.placement(place.context());
        if place == Place::Landscape && placement == DsScreens::default().landscape {
            return Preset::BigTop.placement();
        }
        placement
    }

    pub fn layout_pick(&self, console: LayoutConsole, place: Place) -> Resolved {
        self.layout_pick_from(console, place, Scope::ThisGame, false)
    }

    pub fn game_layout_pick(
        &self,
        key: Option<&str>,
        console: LayoutConsole,
        place: Place,
    ) -> Resolved {
        self.resolve_pick(key, console, place, Scope::ThisGame, false)
    }

    pub fn layout_pick_from(
        &self,
        console: LayoutConsole,
        place: Place,
        games: Scope,
        every_mode: bool,
    ) -> Resolved {
        self.resolve_pick(self.playing_key(), console, place, games, every_mode)
    }

    fn resolve_pick(
        &self,
        key: Option<&str>,
        console: LayoutConsole,
        place: Place,
        games: Scope,
        every_mode: bool,
    ) -> Resolved {
        let picks = self.layout_picks.of(console);
        let levels = [
            (true, false, self.game_pick(key, console, Some(place))),
            (true, true, self.game_pick(key, console, None)),
            (false, false, picks.get(Some(place))),
            (false, true, picks.get(None)),
        ];
        let start = match (games, every_mode) {
            (Scope::ThisGame, false) => 0,
            (Scope::ThisGame, true) => 1,
            (Scope::AllGames, false) => 2,
            (Scope::AllGames, true) => 3,
        };
        levels
            .into_iter()
            .skip(start)
            .find_map(|(game, every_mode, pick)| {
                let pick = pick.filter(|pick| self.usable(console, *pick))?;
                Some(Resolved {
                    pick,
                    game,
                    every_mode,
                    chosen: true,
                })
            })
            .unwrap_or_else(|| Resolved {
                pick: self.default_pick(console, place),
                game: false,
                every_mode: false,
                chosen: false,
            })
    }

    pub fn shown_layout(&self, console: LayoutConsole, place: Place) -> ShownLayout {
        let resolved = self.layout_pick(console, place);
        ShownLayout {
            resolved,
            placement: self.pick_placement(resolved.pick, place),
            spots: self.pick_spots(console, place, resolved.pick),
        }
    }

    pub fn pick_placement(&self, pick: Pick, place: Place) -> Placement {
        let plain = self.ds_screens.placement(place.context());
        match pick {
            Pick::Preset(placement) => placement,
            Pick::Saved(id) => self.saved_layout(id).map_or(plain, |saved| saved.placement),
            Pick::Standard => plain,
        }
    }

    pub fn pick_spots(&self, console: LayoutConsole, place: Place, pick: Pick) -> Spots {
        let Some(builtin) = Builtin::of(pick) else {
            return match pick {
                Pick::Saved(id) => self
                    .saved_layout(id)
                    .map(|saved| saved.spots.clone())
                    .unwrap_or_default(),
                Pick::Preset(_) | Pick::Standard => Spots::new(),
            };
        };
        let Some(edit) = self.builtin_edit(console, place, builtin) else {
            return Spots::new();
        };
        let mut spots = edit.spots.clone();
        if let Pick::Preset(placement) = pick
            && placement.swapped != edit.placement.swapped
        {
            swap_screen_spots(&mut spots);
        }
        spots
    }

    pub fn builtin_edit(
        &self,
        console: LayoutConsole,
        place: Place,
        builtin: Builtin,
    ) -> Option<&SavedLayout> {
        self.saved_layouts.iter().find(|saved| {
            saved.builtin == Some(builtin) && saved.console == console && saved.place == place
        })
    }

    pub fn keep_builtin_edit(
        &mut self,
        console: LayoutConsole,
        place: Place,
        builtin: Builtin,
        placement: Placement,
        spots: Spots,
    ) {
        let kept = self
            .builtin_edit(console, place, builtin)
            .map(|edit| edit.id);
        match (kept, spots.is_empty()) {
            (Some(id), true) => self.reset_builtin(id),
            (Some(id), false) => self.update_saved_layout(id, placement, spots),
            (None, true) => {}
            (None, false) => {
                let id = self.save_layout(builtin.label(), console, place, placement, spots);
                if let Some(edit) = self.saved_layout_mut(id) {
                    edit.builtin = Some(builtin);
                }
            }
        }
    }

    pub fn reset_builtin(&mut self, id: u32) {
        let before = self.saved_layouts.len();
        self.saved_layouts
            .retain(|saved| saved.id != id || saved.builtin.is_none());
        if self.saved_layouts.len() != before {
            self.dirty = true;
        }
    }

    fn game_key(&self, games: Scope) -> Option<String> {
        match games {
            Scope::ThisGame => self.playing_key().map(str::to_owned),
            Scope::AllGames => None,
        }
    }

    pub fn set_layout_pick(
        &mut self,
        console: LayoutConsole,
        place: Place,
        pick: Pick,
        games: Scope,
        every_mode: bool,
    ) {
        let at = (!every_mode).then_some(place);
        match self.game_key(games) {
            Some(key) => {
                let value = serde_json::to_value(pick).unwrap_or_default();
                self.set_own(&key, &pick_path(console, at), value);
                if every_mode {
                    self.drop_own(&key, &pick_path(console, Some(place)));
                }
            }
            None => {
                let picks = self.layout_picks.of_mut(console);
                *picks.slot(at) = Some(pick);
                if every_mode {
                    *picks.slot(Some(place)) = None;
                }
                self.dirty = true;
            }
        }
    }

    pub fn clear_layout_pick(
        &mut self,
        console: LayoutConsole,
        place: Place,
        games: Scope,
        every_mode: bool,
    ) {
        let at = (!every_mode).then_some(place);
        match self.game_key(games) {
            Some(key) => self.drop_own(&key, &pick_path(console, at)),
            None => {
                *self.layout_picks.of_mut(console).slot(at) = None;
                self.dirty = true;
            }
        }
    }

    pub fn act_on_layout(&mut self, console: LayoutConsole, place: Place, action: ScreensAction) {
        let shown = self.shown_layout(console, place);
        let games = if shown.resolved.game {
            Scope::ThisGame
        } else {
            Scope::AllGames
        };
        if action == ScreensAction::Swap {
            self.swap_layout_at(console, place, games, shown.resolved.every_mode);
            return;
        }
        let next = Pick::Preset(shown.placement.act(action));
        self.set_layout_pick(console, place, next, games, shown.resolved.every_mode);
    }

    pub fn swap_layout_at(
        &mut self,
        console: LayoutConsole,
        place: Place,
        games: Scope,
        every_mode: bool,
    ) {
        let resolved = self.layout_pick_from(console, place, games, every_mode);
        let own = resolved.game == (games == Scope::ThisGame) && resolved.every_mode == every_mode;
        if resolved.chosen
            && let Pick::Saved(id) = resolved.pick
            && let Some(saved) = self.saved_layout(id).cloned()
        {
            if own {
                self.swap_saved_layout(id);
                return;
            }
            let name = format!("{} swapped", saved.name);
            let copy = self.save_layout(&name, console, saved.place, saved.placement, saved.spots);
            self.swap_saved_layout(copy);
            self.set_layout_pick(console, place, Pick::Saved(copy), games, every_mode);
            return;
        }
        let now = self.pick_placement(resolved.pick, place);
        let next = Pick::Preset(now.act(ScreensAction::Swap));
        self.set_layout_pick(console, place, next, games, every_mode);
    }

    pub fn pick_name(&self, pick: Pick) -> String {
        match pick {
            Pick::Preset(placement) => Preset::of(placement).label().to_owned(),
            Pick::Standard => STANDARD.to_owned(),
            Pick::Saved(id) => self
                .saved_layout(id)
                .map_or_else(|| STANDARD.to_owned(), |saved| saved.name.clone()),
        }
    }

    pub fn next_layout_name(&self) -> String {
        let taken = |name: &str| self.saved_layouts.iter().any(|saved| saved.name == name);
        if !taken(MIGRATED_NAME) {
            return MIGRATED_NAME.to_owned();
        }
        (2..)
            .map(|n| format!("{MIGRATED_NAME} {n}"))
            .find(|name| !taken(name))
            .unwrap_or_default()
    }

    fn saved_layout_mut(&mut self, id: u32) -> Option<&mut SavedLayout> {
        self.saved_layouts.iter_mut().find(|saved| saved.id == id)
    }

    pub fn update_saved_layout(&mut self, id: u32, placement: Placement, spots: Spots) {
        if let Some(saved) = self.saved_layout_mut(id) {
            saved.placement = placement;
            saved.spots = spots;
            self.dirty = true;
        }
    }

    pub fn rename_layout(&mut self, id: u32, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        if let Some(saved) = self.saved_layout_mut(id) {
            saved.name = name.to_owned();
            self.dirty = true;
        }
    }

    pub fn delete_layout(&mut self, id: u32) {
        self.saved_layouts.retain(|saved| saved.id != id);
        for picks in [
            &mut self.layout_picks.game_boy,
            &mut self.layout_picks.gba,
            &mut self.layout_picks.ds,
        ] {
            for at in std::iter::once(None).chain(Place::iter().map(Some)) {
                let slot = picks.slot(at);
                if *slot == Some(Pick::Saved(id)) {
                    *slot = None;
                }
            }
        }
        self.dirty = true;
    }

    pub fn swap_saved_layout(&mut self, id: u32) {
        let Some(saved) = self.saved_layout_mut(id) else {
            return;
        };
        swap_screen_spots(&mut saved.spots);
        saved.placement.swapped = !saved.placement.swapped;
        self.dirty = true;
    }

    pub fn saved_layouts_in(
        &self,
        console: LayoutConsole,
        place: Place,
    ) -> impl Iterator<Item = &SavedLayout> {
        self.saved_layouts.iter().filter(move |saved| {
            saved.builtin.is_none() && saved.console == console && saved.place == place
        })
    }

    pub fn saved_layout(&self, id: u32) -> Option<&SavedLayout> {
        self.saved_layouts.iter().find(|saved| saved.id == id)
    }

    pub fn save_layout(
        &mut self,
        name: &str,
        console: LayoutConsole,
        place: Place,
        placement: Placement,
        spots: Spots,
    ) -> u32 {
        let id = self.new_layout_id();
        self.saved_layouts.push(SavedLayout {
            id,
            name: name.to_owned(),
            console,
            place,
            placement,
            spots,
            builtin: None,
        });
        self.dirty = true;
        id
    }

    fn new_layout_id(&self) -> u32 {
        for _ in 0..8 {
            let mut bytes = [0u8; 4];
            if getrandom::getrandom(&mut bytes).is_err() {
                break;
            }
            let id = u32::from_le_bytes(bytes);
            if id != 0 && self.saved_layout(id).is_none() {
                return id;
            }
        }
        self.saved_layouts.iter().map(|s| s.id).max().unwrap_or(0) + 1
    }

    pub(crate) fn adopt_screen_layouts(&mut self) {
        if self.layouts_adopted {
            return;
        }
        self.layouts_adopted = true;
        let edited: Vec<(LayoutConsole, LayoutView, Spots)> = self
            .layouts
            .edited_views()
            .map(|(console, view, spots)| (console, view, spots.clone()))
            .collect();
        for (console, view, spots) in edited {
            let place = Place::of_view(view);
            let places: &[Place] = match view {
                LayoutView::Desktop => &[Place::Window, Place::FullScreen],
                _ => &[place],
            };
            let picks = self.layout_picks.of(console);
            if places.iter().all(|at| picks.get(Some(*at)).is_some()) {
                continue;
            }
            let placement = self.ds_screens.placement(place.context());
            let id = self.save_layout(MIGRATED_NAME, console, place, placement, spots);
            self.fill_picks(console, places, Pick::Saved(id));
        }
        let plain = DsScreens::default();
        for (context, places) in [
            (ScreenContext::Portrait, &[Place::Portrait][..]),
            (
                ScreenContext::Landscape,
                &[Place::Landscape, Place::BigScreen],
            ),
            (ScreenContext::Desktop, &[Place::Window, Place::FullScreen]),
        ] {
            let placement = self.ds_screens.placement(context);
            if placement != plain.placement(context) {
                self.fill_picks(LayoutConsole::Ds, places, Pick::Preset(placement));
            }
        }
    }

    fn fill_picks(&mut self, console: LayoutConsole, places: &[Place], pick: Pick) {
        for place in places {
            let slot = self.layout_picks.of_mut(console).slot(Some(*place));
            if slot.is_none() {
                *slot = Some(pick);
                self.dirty = true;
            }
        }
    }
}

fn swap_screen_spots(spots: &mut Spots) {
    let top = spots.remove(&Item::Screen);
    let touch = spots.remove(&Item::TouchScreen);
    if let Some(top) = top {
        spots.insert(Item::TouchScreen, top);
    }
    if let Some(touch) = touch {
        spots.insert(Item::Screen, touch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ui_state::screen_layouts::{Spot, place_screens};
    use egui::{Rect, pos2, vec2};

    const DS: LayoutConsole = LayoutConsole::Ds;

    fn placed(arrangement: Arrangement) -> Placement {
        Placement {
            arrangement,
            ..Placement::default()
        }
    }

    fn spots_in(room: Rect) -> Spots {
        let screen = vec2(256.0, 192.0);
        Spots::from([
            (
                Item::Screen,
                Spot::of(Rect::from_min_size(room.min, screen), room),
            ),
            (
                Item::TouchScreen,
                Spot::of(
                    Rect::from_min_size(room.min + vec2(0.0, 200.0), screen),
                    room,
                ),
            ),
            (
                Item::Dpad,
                Spot::of(
                    Rect::from_min_size(room.min + vec2(20.0, 420.0), vec2(130.0, 130.0)),
                    room,
                ),
            ),
        ])
    }

    #[test]
    fn a_ds_phone_in_landscape_opens_big_top_and_the_big_screen_side_by_side() {
        let mut settings = Settings::default();
        let pick = |settings: &Settings, place| settings.layout_pick(DS, place).pick;
        assert_eq!(
            pick(&settings, Place::Landscape),
            Pick::Preset(Preset::BigTop.placement())
        );
        assert_eq!(
            pick(&settings, Place::BigScreen),
            Pick::Preset(placed(Arrangement::SideBySide))
        );
        settings.ds_screens.landscape = placed(Arrangement::Stacked);
        assert_eq!(
            pick(&settings, Place::Landscape),
            Pick::Preset(placed(Arrangement::Stacked))
        );
    }

    #[test]
    fn a_preset_picked_for_one_place_applies_there_only() {
        let mut settings = Settings::default();
        let big = Pick::Preset(placed(Arrangement::BigAndSmall));
        settings.set_layout_pick(DS, Place::Landscape, big, Scope::AllGames, false);
        assert_eq!(settings.layout_pick(DS, Place::Landscape).pick, big);
        for place in Place::iter().filter(|p| *p != Place::Landscape) {
            let resolved = settings.layout_pick(DS, place);
            assert!(!resolved.chosen, "{place:?}");
            assert_eq!(
                resolved.pick,
                Pick::Preset(settings.ds_screens.placement(place.context()))
            );
        }
        assert_eq!(
            settings
                .layout_pick(LayoutConsole::Gba, Place::Landscape)
                .pick,
            Pick::Standard
        );
    }

    #[test]
    fn saved_layouts_are_listed_in_their_own_place_only() {
        let mut settings = Settings::default();
        let id = settings.save_layout(
            "Thumbs",
            DS,
            Place::Portrait,
            Placement::default(),
            Spots::new(),
        );
        let listed = |s: &Settings, console, place| {
            s.saved_layouts_in(console, place)
                .map(|saved| saved.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(listed(&settings, DS, Place::Portrait), [id]);
        for place in Place::iter().filter(|p| *p != Place::Portrait) {
            assert!(listed(&settings, DS, place).is_empty(), "{place:?}");
        }
        assert!(listed(&settings, LayoutConsole::Gba, Place::Portrait).is_empty());
    }

    #[test]
    fn the_most_specific_level_wins() {
        let mut settings = Settings::default();
        settings.play_game(Some("zelda"));
        let swapped = Placement {
            swapped: true,
            ..Placement::default()
        };
        let picks = [
            placed(Arrangement::Stacked),
            placed(Arrangement::SideBySide),
            placed(Arrangement::BigAndSmall),
            swapped,
        ]
        .map(Pick::Preset);
        let levels = [
            (Scope::AllGames, true),
            (Scope::AllGames, false),
            (Scope::ThisGame, true),
            (Scope::ThisGame, false),
        ];
        for (pick, (games, every_mode)) in picks.into_iter().zip(levels) {
            settings.set_layout_pick(DS, Place::Window, pick, games, every_mode);
            let resolved = settings.layout_pick(DS, Place::Window);
            assert_eq!(resolved.pick, pick, "{games:?} {every_mode}");
            assert_eq!(resolved.game, games == Scope::ThisGame);
            assert_eq!(resolved.every_mode, every_mode);
            assert!(resolved.chosen);
        }
        assert_eq!(
            settings.layout_pick(DS, Place::FullScreen).pick,
            picks[2],
            "this game for every mode reaches the other places"
        );
        for at in (1..levels.len()).rev() {
            let (games, every_mode) = levels[at];
            settings.clear_layout_pick(DS, Place::Window, games, every_mode);
            assert_eq!(settings.layout_pick(DS, Place::Window).pick, picks[at - 1]);
        }
        let (games, every_mode) = levels[0];
        settings.clear_layout_pick(DS, Place::Window, games, every_mode);
        assert!(!settings.layout_pick(DS, Place::Window).chosen);
    }

    #[test]
    fn picking_for_every_mode_drops_this_modes_pick_at_that_level() {
        let mut settings = Settings::default();
        let side = Pick::Preset(placed(Arrangement::SideBySide));
        let big = Pick::Preset(placed(Arrangement::BigAndSmall));
        settings.set_layout_pick(DS, Place::Portrait, side, Scope::AllGames, false);
        settings.set_layout_pick(DS, Place::Landscape, side, Scope::AllGames, false);
        settings.set_layout_pick(DS, Place::Portrait, big, Scope::AllGames, true);
        assert_eq!(settings.layout_pick(DS, Place::Portrait).pick, big);
        assert_eq!(settings.layout_pick(DS, Place::Landscape).pick, side);
    }

    #[test]
    fn a_global_pick_applies_to_games_without_their_own() {
        let mut settings = Settings::default();
        let side = Pick::Preset(placed(Arrangement::SideBySide));
        let big = Pick::Preset(placed(Arrangement::BigAndSmall));
        settings.set_layout_pick(DS, Place::Portrait, side, Scope::AllGames, false);
        settings.play_game(Some("mario"));
        settings.set_layout_pick(DS, Place::Portrait, big, Scope::ThisGame, false);
        assert_eq!(settings.layout_pick(DS, Place::Portrait).pick, big);
        assert!(settings.layout_pick(DS, Place::Portrait).game);
        settings.play_game(Some("kirby"));
        let resolved = settings.layout_pick(DS, Place::Portrait);
        assert_eq!(resolved.pick, side);
        assert!(!resolved.game);
        settings.play_game(Some("mario"));
        assert_eq!(settings.layout_pick(DS, Place::Portrait).pick, big);
        settings.play_game(None);
        assert_eq!(settings.layout_pick(DS, Place::Portrait).pick, side);
        assert_eq!(settings.layout_picks.ds.portrait, Some(side));
        assert!(
            settings
                .own_value("mario", &pick_path(DS, Some(Place::Portrait)))
                .is_some()
        );
        assert!(
            crate::app::ui_state::synced::syncs(&pick_path(DS, Some(Place::Portrait))),
            "a game's own picks follow the account"
        );
    }

    #[test]
    fn a_missing_saved_layout_falls_back_to_the_next_level() {
        let mut settings = Settings::default();
        let side = Pick::Preset(placed(Arrangement::SideBySide));
        settings.set_layout_pick(DS, Place::Portrait, side, Scope::AllGames, true);
        settings.set_layout_pick(DS, Place::Portrait, Pick::Saved(77), Scope::AllGames, false);
        assert_eq!(settings.layout_pick(DS, Place::Portrait).pick, side);
        let gba = settings.save_layout(
            "Wide",
            LayoutConsole::Gba,
            Place::Portrait,
            Placement::default(),
            Spots::new(),
        );
        settings.set_layout_pick(
            DS,
            Place::Portrait,
            Pick::Saved(gba),
            Scope::AllGames,
            false,
        );
        assert_eq!(settings.layout_pick(DS, Place::Portrait).pick, side);
    }

    #[test]
    fn a_saved_layout_used_for_every_mode_fits_inside_another_place() {
        let mut settings = Settings::default();
        let portrait = Rect::from_min_size(pos2(0.0, 0.0), vec2(412.0, 800.0));
        let spots = spots_in(portrait);
        let id = settings.save_layout(
            "Tall",
            DS,
            Place::Portrait,
            Placement::default(),
            spots.clone(),
        );
        settings.set_layout_pick(DS, Place::Portrait, Pick::Saved(id), Scope::AllGames, true);
        let screen = vec2(256.0, 192.0);
        for room in [
            Rect::from_min_size(pos2(74.0, 0.0), vec2(841.0, 364.0)),
            Rect::from_min_size(pos2(0.0, 40.0), vec2(1280.0, 680.0)),
            Rect::from_min_size(pos2(0.0, 0.0), vec2(960.0, 540.0)),
        ] {
            for place in [Place::Landscape, Place::Window, Place::BigScreen] {
                let shown = settings.shown_layout(DS, place);
                assert_eq!(shown.spots, spots, "{place:?}");
                let screens = place_screens(
                    &[
                        (shown.spots[&Item::Screen], screen),
                        (shown.spots[&Item::TouchScreen], screen),
                    ],
                    room,
                );
                for rect in &screens {
                    assert!(room.expand(0.01).contains_rect(*rect), "{room:?}: {rect:?}");
                }
                assert!(!screens[0].shrink(0.01).intersects(screens[1].shrink(0.01)));
                let dpad = shown.spots[&Item::Dpad].rect(room, vec2(130.0, 130.0));
                assert!(room.expand(0.01).contains_rect(dpad), "{room:?}: {dpad:?}");
            }
        }
    }

    #[test]
    fn swapping_changes_the_pick_at_the_level_it_comes_from() {
        let mut settings = Settings::default();
        settings.play_game(Some("metroid"));
        let big = Pick::Preset(placed(Arrangement::BigAndSmall));
        settings.set_layout_pick(DS, Place::Landscape, big, Scope::ThisGame, true);
        settings.act_on_layout(DS, Place::Landscape, ScreensAction::Swap);
        let resolved = settings.layout_pick(DS, Place::Landscape);
        assert!(resolved.game && resolved.every_mode);
        assert_eq!(
            Preset::of(settings.shown_layout(DS, Place::Landscape).placement),
            Preset::BigBottom
        );
        assert_eq!(settings.layout_picks, LayoutPicks::default());
        settings.play_game(None);
        settings.act_on_layout(DS, Place::Portrait, ScreensAction::OneScreen);
        let resolved = settings.layout_pick(DS, Place::Portrait);
        assert!(resolved.chosen && !resolved.game && !resolved.every_mode);
        assert_eq!(
            Preset::of(settings.shown_layout(DS, Place::Portrait).placement),
            Preset::TopOnly
        );
        assert_eq!(settings.ds_screens, DsScreens::default());
    }

    #[test]
    fn every_preset_lights_its_own_card() {
        for preset in Preset::iter() {
            assert_eq!(Preset::of(preset.placement()), preset, "{}", preset.label());
        }
        let swapped_side = Placement {
            swapped: true,
            ..placed(Arrangement::SideBySide)
        };
        assert_eq!(Preset::of(swapped_side), Preset::SideBySide);
    }

    #[test]
    fn picks_keep_every_key_when_saved() {
        let saved = serde_json::to_value(LayoutPicks::default()).unwrap();
        for console in [LayoutConsole::GameBoy, LayoutConsole::Gba, DS] {
            for at in Place::iter().map(Some).chain([None]) {
                let path = pick_path(console, at);
                let inner = path.strip_prefix("layout_picks.").unwrap();
                let found = inner.split('.').try_fold(&saved, |v, part| v.get(part));
                assert_eq!(found, Some(&serde_json::Value::Null), "{path}");
            }
        }
    }

    fn old_settings() -> Settings {
        let mut settings = Settings {
            layouts_adopted: false,
            ..Settings::default()
        };
        settings.ds_screens.desktop = placed(Arrangement::SideBySide);
        settings.ds_screens.portrait = Placement {
            swapped: true,
            ..Placement::default()
        };
        let room = Rect::from_min_size(pos2(0.0, 0.0), vec2(412.0, 800.0));
        for (item, spot) in spots_in(room) {
            settings.layouts.set(DS, LayoutView::Portrait, item, spot);
            settings
                .layouts
                .set(LayoutConsole::Gba, LayoutView::Desktop, item, spot);
        }
        settings
    }

    #[test]
    fn edited_layouts_and_placements_become_picks_once() {
        let mut settings = old_settings();
        settings.adopt_screen_layouts();
        assert_eq!(settings.saved_layouts.len(), 2);
        let portrait = settings.layout_pick(DS, Place::Portrait);
        let Pick::Saved(id) = portrait.pick else {
            panic!("{portrait:?}");
        };
        let saved = settings.saved_layout(id).unwrap();
        assert_eq!(saved.name, MIGRATED_NAME);
        assert_eq!(saved.place, Place::Portrait);
        assert!(saved.placement.swapped);
        assert_eq!(
            saved.spots,
            settings.layouts.spots(DS, LayoutView::Portrait)
        );
        let side = Pick::Preset(placed(Arrangement::SideBySide));
        assert_eq!(settings.layout_picks.ds.window, Some(side));
        assert_eq!(settings.layout_picks.ds.full_screen, Some(side));
        assert_eq!(settings.layout_picks.ds.landscape, None);
        let gba = settings.layout_pick(LayoutConsole::Gba, Place::Window).pick;
        assert_eq!(
            settings
                .layout_pick(LayoutConsole::Gba, Place::FullScreen)
                .pick,
            gba
        );
        assert!(matches!(gba, Pick::Saved(_)));
        assert_eq!(settings.ds_screens, old_settings().ds_screens);
        let once = serde_json::to_value(&settings).unwrap();
        settings.adopt_screen_layouts();
        assert_eq!(serde_json::to_value(&settings).unwrap(), once);
    }

    #[test]
    fn loading_old_settings_adopts_their_layouts() {
        let stored = serde_json::to_value(old_settings()).unwrap();
        let loaded: Settings = serde_json::from_value(stored).unwrap();
        assert_eq!(loaded.saved_layouts.len(), 2);
        let again: Settings =
            serde_json::from_value(serde_json::to_value(&loaded).unwrap()).unwrap();
        assert_eq!(again.saved_layouts, loaded.saved_layouts);
        assert_eq!(again.layout_picks, loaded.layout_picks);
    }

    #[test]
    fn adopting_fills_only_empty_picks_and_keeps_the_synced_placements() {
        let mut settings = old_settings();
        let top = Pick::Preset(Preset::TopOnly.placement());
        settings.layout_picks.ds.portrait = Some(top);
        settings.layout_picks.ds.window = Some(top);
        settings.adopt_screen_layouts();
        assert_eq!(settings.layout_picks.ds.portrait, Some(top));
        assert_eq!(settings.layout_picks.ds.window, Some(top));
        assert_eq!(
            settings.layout_picks.ds.full_screen,
            Some(Pick::Preset(placed(Arrangement::SideBySide)))
        );
        assert_eq!(settings.saved_layouts.len(), 1, "only the GBA layout");
        assert_eq!(settings.saved_layouts[0].console, LayoutConsole::Gba);
        assert_eq!(settings.ds_screens, old_settings().ds_screens);
    }

    #[test]
    fn after_adopting_a_sync_or_a_reload_makes_no_layout() {
        let stored = serde_json::to_value(old_settings()).unwrap();
        let mut settings: Settings = serde_json::from_value(stored).unwrap();
        assert!(settings.layouts_adopted);
        let saved = settings.saved_layouts.clone();
        let picks = settings.layout_picks;
        let room = Rect::from_min_size(pos2(0.0, 0.0), vec2(915.0, 412.0));
        for (item, spot) in spots_in(room) {
            settings.layouts.set(DS, LayoutView::Landscape, item, spot);
        }
        let reloaded: Settings =
            serde_json::from_value(serde_json::to_value(&settings).unwrap()).unwrap();
        assert_eq!(reloaded.saved_layouts, saved);
        assert_eq!(reloaded.layout_picks, picks);

        let mut desk = Settings {
            matrix: true,
            layout_picks: picks,
            ..Settings::default()
        };
        desk.ds_screens.landscape = placed(Arrangement::SideBySide);
        let values = crate::app::ui_state::synced::synced_values(&desk);
        assert!(crate::app::ui_state::synced::adopt(&mut settings, &values));
        assert!(settings.matrix);
        assert_eq!(settings.saved_layouts, saved);
        assert_eq!(settings.layout_picks, picks);
        assert_eq!(
            settings.ds_screens.landscape,
            placed(Arrangement::SideBySide)
        );
        assert!(settings.layouts.edited(DS, LayoutView::Landscape));
    }

    #[test]
    fn unknown_picks_and_arrangements_from_a_newer_client_are_skipped() {
        let mut values = crate::app::ui_state::synced::synced_values(&Settings::default());
        values.insert(
            "layout_picks".to_owned(),
            serde_json::json!({
                "ds": {
                    "portrait": {"Mosaic": 3},
                    "landscape": {"Preset": {"arrangement": "Diagonal", "swapped": true}},
                    "window": {"Preset": {"arrangement": "SideBySide"}},
                }
            }),
        );
        values.insert(
            "ds_screens".to_owned(),
            serde_json::json!({"desktop": {"arrangement": "Diagonal", "one_screen": true}}),
        );
        values.insert("matrix".to_owned(), serde_json::json!(true));
        let mut settings = Settings::default();
        assert!(crate::app::ui_state::synced::adopt(&mut settings, &values));
        assert!(settings.matrix);
        let picks = settings.layout_picks.ds;
        assert_eq!(picks.portrait, None);
        assert_eq!(
            picks.landscape,
            Some(Pick::Preset(Placement {
                swapped: true,
                ..Placement::default()
            }))
        );
        assert_eq!(
            picks.window,
            Some(Pick::Preset(placed(Arrangement::SideBySide)))
        );
        assert_eq!(
            settings.ds_screens.desktop.arrangement,
            Arrangement::Stacked
        );
        assert!(settings.ds_screens.desktop.one_screen);
    }

    #[test]
    fn swapping_at_this_game_over_a_shared_saved_layout_leaves_it_alone() {
        let mut settings = Settings::default();
        settings.play_game(Some("zelda"));
        let room = Rect::from_min_size(pos2(0.0, 0.0), vec2(412.0, 800.0));
        let id = settings.save_layout(
            "Thumbs",
            DS,
            Place::Portrait,
            Placement::default(),
            spots_in(room),
        );
        settings.set_layout_pick(DS, Place::Portrait, Pick::Saved(id), Scope::AllGames, false);
        let shared = settings.saved_layout(id).cloned().unwrap();
        settings.swap_layout_at(DS, Place::Portrait, Scope::ThisGame, false);
        assert_eq!(settings.saved_layout(id), Some(&shared));
        let resolved = settings.layout_pick(DS, Place::Portrait);
        assert!(resolved.game && !resolved.every_mode);
        let Pick::Saved(copy) = resolved.pick else {
            panic!("{resolved:?}");
        };
        assert_ne!(copy, id);
        let swapped = settings.saved_layout(copy).unwrap();
        assert_eq!(swapped.name, "Thumbs swapped");
        assert!(swapped.placement.swapped);
        assert_eq!(
            swapped.spots[&Item::Screen],
            shared.spots[&Item::TouchScreen]
        );
        settings.play_game(Some("kirby"));
        assert_eq!(
            settings.layout_pick(DS, Place::Portrait).pick,
            Pick::Saved(id)
        );
        settings.swap_layout_at(DS, Place::Portrait, Scope::AllGames, false);
        assert!(settings.saved_layout(id).unwrap().placement.swapped);
        assert_eq!(settings.saved_layouts.len(), 2);
    }
}

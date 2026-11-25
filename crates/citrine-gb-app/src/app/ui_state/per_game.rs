use crate::app::ui_state::palette::PaletteChoice;
use crate::app::ui_state::settings::{ConsoleChoice, Settings};
use crate::app::ui_state::settings_tree::Surface;
use crate::app::ui_state::settings_tree::rows::RowId;
use citrine_gb::console::ConsoleKind;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::OnceLock;
use strum::IntoEnumIterator;

#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct PerGame(BTreeMap<String, BTreeMap<String, Value>>);

#[derive(Debug, Default)]
pub struct Playing {
    key: Option<String>,
    title: String,
    console: Option<ConsoleKind>,
    globals: BTreeMap<String, Value>,
    scope: Scope,
    drops: Vec<(RowId, Surface)>,
    shown: BTreeMap<RowId, String>,
}

pub fn per_game_paths() -> &'static [&'static str] {
    static PATHS: OnceLock<Vec<&'static str>> = OnceLock::new();
    PATHS.get_or_init(|| {
        let mut paths: Vec<&'static str> = RowId::iter()
            .filter(|row| row.per_game())
            .flat_map(|row| Surface::iter().flat_map(move |surface| row.field().paths(surface)))
            .collect();
        paths.sort_unstable();
        paths.dedup();
        paths
    })
}

pub fn is_per_game(path: &str) -> bool {
    per_game_paths().binary_search(&path).is_ok()
}

macro_rules! fields {
    ($($name:ident),* $(,)?) => {
        #[cfg(test)]
        const FIELDS: &[&str] = &[$(stringify!($name)),*];

        fn field_value(settings: &Settings, name: &str) -> Option<Value> {
            match name {
                $(stringify!($name) => serde_json::to_value(&settings.$name).ok(),)*
                _ => None,
            }
        }

        fn set_field_value(settings: &mut Settings, name: &str, value: Value) -> bool {
            match name {
                $(stringify!($name) => match serde_json::from_value(value) {
                    Ok(next) => {
                        settings.$name = next;
                        true
                    }
                    Err(_) => false,
                },)*
                _ => false,
            }
        }
    };
}

fields!(
    ui_scale,
    ui_scale_per_ratio,
    ui_theme,
    layout,
    console,
    game_names,
    cheats,
    dmg_theme,
    custom_palette,
    gbc_palette,
    palettes,
    per_game,
    per_game_local,
    color_correction,
    gba_color_correction,
    scaling,
    same_size_pixels,
    ds_screens,
    ds_3d,
    ds_jit,
    gba_jit,
    matrix,
    ghosting,
    match_screen,
    matrix_edge_darkness,
    matrix_corner_darkness,
    ghosting_strength,
    volume,
    muted,
    microphone,
    push_to_talk,
    talk_button,
    audio_queue_ms,
    audio_device,
    slow_sound,
    fast_forward_sound,
    channels,
    gba_channels,
    bindings,
    hotkeys,
    touch,
    layouts,
    layout_picks,
    saved_layouts,
    layouts_adopted,
    online,
    local_link,
    gimmicks,
    discord,
    achievements,
    stats_range,
    current_tab,
    quick_slot,
    autosave_interval,
    autosave_keep,
    rewind,
    fast_forward_speed,
    fast_forward_key,
    quick_load,
    background_pause,
    screenshot_scale,
    clip_length,
    clip_length_adopted,
    capture_folder,
    recording_scale,
    record_frame_times,
    render_video,
    randomized_ram,
    dev_mode,
    check_updates,
    covers,
    fullscreen,
    hide_pointer,
    library_depth,
    focus_place,
    hud,
    big_screen,
    ds_user,
    ds_online,
    walker,
    track_pc,
    synced,
);

fn split(path: &str) -> (&str, Option<&str>) {
    match path.split_once('.') {
        Some((top, rest)) => (top, Some(rest)),
        None => (path, None),
    }
}

fn lookup<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .try_fold(root, |value, part| value.get(part))
}

fn lookup_mut<'a>(root: &'a mut Value, path: &str) -> Option<&'a mut Value> {
    path.split('.')
        .try_fold(root, |value, part| value.get_mut(part))
}

pub(crate) fn get(settings: &Settings, path: &str) -> Option<Value> {
    let (top, rest) = split(path);
    let value = field_value(settings, top)?;
    match rest {
        None => Some(value),
        Some(rest) => lookup(&value, rest).cloned(),
    }
}

pub(crate) fn set(settings: &mut Settings, path: &str, next: Value) -> bool {
    let (top, rest) = split(path);
    let value = match rest {
        None => next,
        Some(rest) => {
            let Some(mut value) = field_value(settings, top) else {
                return false;
            };
            let Some(slot) = lookup_mut(&mut value, rest) else {
                return false;
            };
            *slot = next;
            value
        }
    };
    set_field_value(settings, top, value)
}

struct Raw<'a>(&'a Settings);

impl serde::Serialize for Raw<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Settings::serialize(self.0, serializer)
    }
}

impl serde::Serialize for Settings {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.playing.globals.is_empty() {
            return Settings::serialize(self, serializer);
        }
        let mut stored = serde_json::to_value(Raw(self)).map_err(serde::ser::Error::custom)?;
        for (path, global) in &self.playing.globals {
            if let Some(slot) = lookup_mut(&mut stored, path) {
                *slot = global.clone();
            }
        }
        serde::Serialize::serialize(&stored, serializer)
    }
}

impl<'de> serde::Deserialize<'de> for Settings {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut settings = Settings::deserialize(deserializer)?;
        settings.adopt_game_palettes();
        settings.adopt_game_consoles();
        settings.adopt_screen_layouts();
        settings.adopt_clip_length();
        Ok(settings)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    #[default]
    AllGames,
    ThisGame,
}

#[must_use]
pub enum ScopeFrame {
    Off,
    AllGames,
    ThisGame(BTreeMap<&'static str, Value>),
}

fn row_groups() -> &'static [Vec<&'static str>] {
    static GROUPS: OnceLock<Vec<Vec<&'static str>>> = OnceLock::new();
    GROUPS.get_or_init(|| {
        let mut groups: Vec<Vec<&'static str>> = RowId::iter()
            .filter(|row| row.per_game())
            .flat_map(|row| Surface::iter().map(move |surface| row.field().paths(surface)))
            .filter(|paths| !paths.is_empty())
            .collect();
        groups.sort_unstable();
        groups.dedup();
        groups
    })
}

impl PerGame {
    fn own(&self, key: &str) -> impl Iterator<Item = (&String, &Value)> {
        self.0.get(key).into_iter().flatten()
    }

    fn insert(&mut self, key: &str, path: &str, value: Value) -> bool {
        let own = self.0.entry(key.to_owned()).or_default();
        own.insert(path.to_owned(), value.clone()) != Some(value)
    }

    fn remove(&mut self, key: &str, path: &str) -> bool {
        let Some(own) = self.0.get_mut(key) else {
            return false;
        };
        let removed = own.remove(path).is_some();
        if own.is_empty() {
            self.0.remove(key);
        }
        removed
    }
}

impl Settings {
    pub fn playing_key(&self) -> Option<&str> {
        self.playing.key.as_deref()
    }

    pub fn playing_title(&self) -> &str {
        &self.playing.title
    }

    pub fn name_game(&mut self, title: &str) {
        if self.playing.title != title {
            self.playing.title = title.to_owned();
        }
    }

    pub fn playing_console(&self) -> Option<ConsoleKind> {
        self.playing.console
    }

    pub fn name_console(&mut self, console: Option<ConsoleKind>) {
        self.playing.console = console;
    }

    pub fn game_text(&self, row: RowId) -> Option<&str> {
        self.playing.shown.get(&row).map(String::as_str)
    }

    pub fn drop_later(&mut self, row: RowId, surface: Surface) {
        self.playing.drops.push((row, surface));
    }

    pub fn scope(&self) -> Scope {
        match self.playing.key {
            Some(_) => self.playing.scope,
            None => Scope::AllGames,
        }
    }

    pub fn set_scope(&mut self, scope: Scope) {
        self.playing.scope = scope;
    }

    pub fn leave_scope(&mut self) -> bool {
        let left = self.scope() == Scope::ThisGame;
        self.playing.scope = Scope::AllGames;
        left
    }

    pub fn play_game(&mut self, key: Option<&str>) {
        if self.playing.key.as_deref() == key {
            self.settle();
            return;
        }
        self.leave_game();
        self.playing.scope = Scope::AllGames;
        let Some(key) = key else {
            return;
        };
        self.playing.key = Some(key.to_owned());
        self.wear_own();
        self.dirty = true;
    }

    pub(crate) fn resume_game(&mut self, old: &mut Settings) {
        let playing = std::mem::take(&mut old.playing);
        self.playing.title = playing.title;
        self.playing.console = playing.console;
        let Some(key) = playing.key else {
            return;
        };
        self.playing.key = Some(key);
        self.playing.scope = playing.scope;
        self.wear_own();
    }

    fn wear_own(&mut self) {
        let Some(key) = self.playing.key.clone() else {
            return;
        };
        let own: Vec<(String, Value)> = self
            .own_values(&key)
            .filter(|(path, _)| is_per_game(path))
            .map(|(path, value)| (path.to_owned(), value.clone()))
            .collect();
        for (path, value) in own {
            self.wear(&path, value);
        }
    }

    fn leave_game(&mut self) {
        self.settle();
        for (path, global) in std::mem::take(&mut self.playing.globals) {
            set(self, &path, global);
        }
        if self.playing.key.take().is_some() {
            self.dirty = true;
        }
    }

    fn settle(&mut self) {
        let Some(key) = self.playing.key.clone() else {
            return;
        };
        let worn: Vec<String> = self.playing.globals.keys().cloned().collect();
        for path in worn {
            if let Some(value) = get(self, &path) {
                self.dirty |= self.store(&key, &path, value);
            }
        }
    }

    fn wear(&mut self, path: &str, value: Value) {
        let Some(global) = get(self, path) else {
            return;
        };
        if set(self, path, value) {
            self.playing
                .globals
                .entry(path.to_owned())
                .or_insert(global);
        }
    }

    fn store(&mut self, key: &str, path: &str, value: Value) -> bool {
        if super::synced::syncs(path) {
            self.per_game.insert(key, path, value)
        } else {
            self.per_game_local.insert(key, path, value)
        }
    }

    pub fn own_values(&self, key: &str) -> impl Iterator<Item = (&str, &Value)> {
        self.per_game
            .own(key)
            .chain(self.per_game_local.own(key))
            .map(|(path, value)| (path.as_str(), value))
    }

    pub fn own_value(&self, key: &str, path: &str) -> Option<&Value> {
        self.per_game
            .0
            .get(key)
            .and_then(|own| own.get(path))
            .or_else(|| self.per_game_local.0.get(key).and_then(|own| own.get(path)))
    }

    pub fn global_value(&self, path: &str) -> Option<Value> {
        self.playing
            .globals
            .get(path)
            .cloned()
            .or_else(|| get(self, path))
    }

    pub fn value_for<T: serde::de::DeserializeOwned>(&self, key: &str, path: &str) -> Option<T> {
        let value = match self.own_value(key, path) {
            Some(own) => own.clone(),
            None => self.global_value(path)?,
        };
        serde_json::from_value(value).ok()
    }

    #[cfg(feature = "nds")]
    pub fn boot_languages(&self) -> crate::app::ui_state::ds_user::BootLanguages {
        use crate::app::ui_state::ds_user::{BootLanguages, DsLanguage};
        const PATH: &str = "ds_user.language";
        let app = DsLanguage::of_app();
        let language = |value: Value| {
            serde_json::from_value::<Option<DsLanguage>>(value)
                .ok()
                .flatten()
                .unwrap_or(app)
        };
        BootLanguages {
            all: self.global_value(PATH).map_or(app, &language),
            games: self
                .per_game
                .0
                .iter()
                .chain(&self.per_game_local.0)
                .filter_map(|(key, own)| Some((key.clone(), language(own.get(PATH)?.clone()))))
                .collect(),
        }
    }

    pub fn owns(&self, row: RowId, surface: Surface) -> bool {
        let Some(key) = self.playing_key() else {
            return false;
        };
        row.per_game()
            && row
                .field()
                .paths(surface)
                .iter()
                .any(|path| self.own_value(key, path).is_some())
    }

    pub fn owned_rows(&self, surface: Surface) -> impl Iterator<Item = RowId> + '_ {
        RowId::iter().filter(move |row| self.owns(*row, surface))
    }

    pub fn set_own(&mut self, key: &str, path: &str, value: Value) {
        self.store(key, path, value.clone());
        if self.playing.key.as_deref() == Some(key) && is_per_game(path) {
            self.wear(path, value);
        }
        self.dirty = true;
    }

    pub fn drop_own(&mut self, key: &str, path: &str) {
        let removed = self.per_game.remove(key, path) | self.per_game_local.remove(key, path);
        if self.playing.key.as_deref() == Some(key)
            && let Some(global) = self.playing.globals.remove(path)
        {
            set(self, path, global);
        }
        self.dirty |= removed;
    }

    pub fn drop_row(&mut self, row: RowId, surface: Surface) {
        let Some(key) = self.playing.key.clone() else {
            return;
        };
        for path in row.field().paths(surface) {
            self.drop_own(&key, path);
        }
    }

    pub fn begin_scope(&mut self, shown: BTreeMap<RowId, String>) -> ScopeFrame {
        self.begin_scope_as(shown, self.playing.scope)
    }

    pub fn begin_scope_as(&mut self, shown: BTreeMap<RowId, String>, scope: Scope) -> ScopeFrame {
        self.playing.shown = shown;
        if self.playing.key.is_none() {
            return ScopeFrame::Off;
        }
        match scope {
            Scope::AllGames => {
                self.settle();
                for (path, global) in std::mem::take(&mut self.playing.globals) {
                    set(self, &path, global);
                }
                ScopeFrame::AllGames
            }
            Scope::ThisGame => ScopeFrame::ThisGame(
                per_game_paths()
                    .iter()
                    .filter_map(|path| Some((*path, get(self, path)?)))
                    .collect(),
            ),
        }
    }

    pub fn end_scope(&mut self, frame: ScopeFrame) {
        self.end_frame(frame);
        for (row, surface) in std::mem::take(&mut self.playing.drops) {
            self.drop_row(row, surface);
        }
    }

    fn end_frame(&mut self, frame: ScopeFrame) {
        let Some(key) = self.playing.key.clone() else {
            return;
        };
        match frame {
            ScopeFrame::Off => {}
            ScopeFrame::AllGames => self.wear_own(),
            ScopeFrame::ThisGame(before) => {
                for group in row_groups() {
                    let changed = group
                        .iter()
                        .any(|path| before.get(path) != get(self, path).as_ref());
                    if !changed {
                        continue;
                    }
                    for path in group {
                        let (Some(was), Some(now)) = (before.get(path), get(self, path)) else {
                            continue;
                        };
                        self.playing
                            .globals
                            .entry((*path).to_owned())
                            .or_insert_with(|| was.clone());
                        self.store(&key, path, now);
                    }
                    if group
                        .iter()
                        .all(|path| self.global_value(path) == get(self, path))
                    {
                        for path in group {
                            self.drop_own(&key, path);
                        }
                    }
                    self.dirty = true;
                }
            }
        }
    }

    pub(crate) fn rename_own_palettes(&mut self, from: &str, to: &str) {
        let from = Value::String(from.to_owned());
        let worn = self.playing.globals.get_mut("custom_palette");
        if let Some(global) = worn
            && *global == from
        {
            *global = Value::String(to.to_owned());
        }
        for own in self.per_game.0.values_mut() {
            if let Some(name) = own.get_mut("custom_palette")
                && *name == from
            {
                *name = Value::String(to.to_owned());
            }
        }
    }

    fn adopt_game_palettes(&mut self) {
        let palettes = std::mem::take(&mut self.game_palettes);
        for (key, choice) in palettes {
            let dmg_theme = serde_json::to_value(self.dmg_theme).unwrap_or_default();
            let custom = serde_json::to_value(&self.custom_palette).unwrap_or_default();
            let values = match choice {
                PaletteChoice::Builtin(theme) => [
                    serde_json::to_value(theme).unwrap_or_default(),
                    Value::Null,
                    Value::Null,
                ],
                PaletteChoice::Custom(name) => [dmg_theme, Value::String(name), Value::Null],
                PaletteChoice::Gbc(palette) => [
                    dmg_theme,
                    custom,
                    serde_json::to_value(palette).unwrap_or_default(),
                ],
            };
            for (path, value) in ["dmg_theme", "custom_palette", "gbc_palette"]
                .into_iter()
                .zip(values)
            {
                if self.own_value(&key, path).is_none() {
                    self.store(&key, path, value);
                }
            }
        }
    }

    fn adopt_game_consoles(&mut self) {
        for (key, model) in std::mem::take(&mut self.game_consoles) {
            if self.own_value(&key, "console").is_none() {
                let choice = serde_json::to_value(ConsoleChoice::of(model)).unwrap_or_default();
                self.store(&key, "console", choice);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb::gb::ppu::types::theme::DmgTheme;

    fn on(value: bool) -> Value {
        Value::Bool(value)
    }

    #[test]
    fn every_stored_field_can_be_read_and_written_by_path() {
        let stored = serde_json::to_value(Settings::default()).expect("settings save");
        let mut keys: Vec<&str> = stored
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        let mut fields = FIELDS.to_vec();
        fields.sort_unstable();
        assert_eq!(fields, keys);

        let mut settings = Settings::default();
        for path in per_game_paths() {
            let value = get(&settings, path).unwrap_or_else(|| panic!("{path} reads"));
            assert!(set(&mut settings, path, value), "{path} writes");
        }
    }

    #[test]
    fn a_games_own_value_applies_while_it_runs_and_the_global_one_on_the_next_game() {
        let mut settings = Settings {
            ghosting: false,
            ..Default::default()
        };
        settings.set_own("porklike", "ghosting", on(true));
        assert!(!settings.ghosting);

        settings.play_game(Some("porklike"));
        assert!(settings.ghosting);
        settings.play_game(Some("tobu"));
        assert!(!settings.ghosting);
        settings.play_game(Some("porklike"));
        assert!(settings.ghosting);
        settings.play_game(None);
        assert!(!settings.ghosting);
    }

    #[test]
    fn saving_while_a_game_runs_keeps_the_global_value() {
        let mut settings = Settings::default();
        settings.set_own("porklike", "ghosting", on(true));
        settings.play_game(Some("porklike"));
        let saved = serde_json::to_value(&settings).expect("settings save");
        assert_eq!(saved["ghosting"], on(false));
        assert_eq!(saved["per_game"]["porklike"]["ghosting"], on(true));

        let mut back: Settings = serde_json::from_value(saved).expect("settings load");
        assert!(!back.ghosting);
        back.play_game(Some("porklike"));
        assert!(back.ghosting);
    }

    #[test]
    fn dropping_a_games_value_brings_back_the_global_one() {
        let mut settings = Settings {
            matrix_edge_darkness: 0.4,
            ..Default::default()
        };
        settings.set_own("porklike", "matrix_edge_darkness", serde_json::json!(0.9));
        settings.play_game(Some("porklike"));
        assert_eq!(settings.matrix_edge_darkness, 0.9);
        settings.drop_own("porklike", "matrix_edge_darkness");
        assert_eq!(settings.matrix_edge_darkness, 0.4);
        assert_eq!(settings.own_values("porklike").count(), 0);
    }

    #[test]
    fn a_nested_path_keeps_its_neighbours() {
        let mut settings = Settings::default();
        let path = "ds_screens.rotation";
        let before = settings.ds_screens;
        let turned = crate::app::ui_state::ds_screens::Rotation::Right;
        settings.set_own("pong", path, serde_json::to_value(turned).unwrap());
        settings.play_game(Some("pong"));
        assert_eq!(settings.ds_screens.rotation, turned);
        assert_eq!(settings.ds_screens.gap, before.gap);
        settings.play_game(None);
        assert_eq!(settings.ds_screens, before);
    }

    #[test]
    fn per_game_palettes_from_0_12_carry_over() {
        let old = r#"{
            "dmg_theme": "GreyScale",
            "custom_palette": "Mine",
            "palettes": {"entries": [{"name": "Mine", "colors": [[1,2,3],[4,5,6],[7,8,9],[10,11,12]]}]},
            "game_palettes": {
                "aaa": {"Builtin": "Aqua"},
                "bbb": {"Custom": "Mine"}
            }
        }"#;
        let mut settings: Settings = serde_json::from_str(old).expect("0.12 settings load");
        assert_eq!(
            settings.global_choice(),
            PaletteChoice::Custom("Mine".into())
        );

        settings.play_game(Some("aaa"));
        assert_eq!(
            settings.global_choice(),
            PaletteChoice::Builtin(DmgTheme::Aqua)
        );
        settings.play_game(Some("bbb"));
        assert_eq!(
            settings.global_choice(),
            PaletteChoice::Custom("Mine".into())
        );
        settings.play_game(None);

        let saved = serde_json::to_value(&settings).expect("settings save");
        assert!(saved.get("game_palettes").is_none());
        let back: Settings = serde_json::from_value(saved).expect("settings load");
        assert_eq!(back.own_values("aaa").count(), 3);
    }

    fn shown() -> BTreeMap<RowId, String> {
        BTreeMap::new()
    }

    #[test]
    fn an_edit_for_all_games_keeps_the_games_own_value() {
        let mut settings = Settings::default();
        settings.set_own("porklike", "ghosting", on(true));
        settings.play_game(Some("porklike"));
        assert!(settings.ghosting);

        let frame = settings.begin_scope(shown());
        assert!(!settings.ghosting, "all games shows the global value");
        settings.ghosting = false;
        settings.matrix = true;
        settings.end_scope(frame);

        assert!(settings.ghosting, "the game keeps its own value");
        assert!(settings.matrix);
        assert!(!settings.owns(RowId::LcdGrid, Surface::Desktop));
        settings.play_game(None);
        assert!(settings.matrix, "the edit went to all games");
        assert!(!settings.ghosting);
    }

    #[test]
    fn an_edit_for_this_game_becomes_the_games_own_value() {
        let mut settings = Settings::default();
        settings.play_game(Some("porklike"));
        settings.set_scope(Scope::ThisGame);

        let frame = settings.begin_scope(shown());
        settings.matrix = true;
        settings.end_scope(frame);

        assert!(settings.matrix);
        assert!(settings.owns(RowId::LcdGrid, Surface::Desktop));
        assert!(!settings.owns(RowId::Ghosting, Surface::Desktop));
        settings.play_game(Some("tobu"));
        assert!(!settings.matrix, "another game keeps the global value");
        assert_eq!(settings.scope(), Scope::AllGames);
        settings.play_game(Some("porklike"));
        assert!(settings.matrix);
    }

    #[test]
    fn a_value_set_back_to_all_games_stops_being_the_games_own() {
        let mut settings = Settings::default();
        settings.play_game(Some("porklike"));
        settings.set_scope(Scope::ThisGame);

        let frame = settings.begin_scope(shown());
        settings.matrix = true;
        settings.end_scope(frame);
        let frame = settings.begin_scope(shown());
        settings.matrix = false;
        settings.end_scope(frame);

        assert!(!settings.owns(RowId::LcdGrid, Surface::Desktop));
        assert_eq!(settings.own_values("porklike").count(), 0);
    }

    #[test]
    fn a_palette_set_for_this_game_owns_all_its_parts() {
        let mut settings = Settings {
            custom_palette: Some("Mine".into()),
            ..Default::default()
        };
        settings.play_game(Some("porklike"));
        settings.set_scope(Scope::ThisGame);

        let frame = settings.begin_scope(shown());
        settings.set_global_choice(PaletteChoice::Builtin(DmgTheme::Aqua));
        settings.end_scope(frame);

        let own: Vec<&str> = settings
            .own_values("porklike")
            .map(|(path, _)| path)
            .collect();
        assert_eq!(own, ["custom_palette", "dmg_theme", "gbc_palette"]);
        settings.play_game(None);
        assert_eq!(
            settings.global_choice(),
            PaletteChoice::Custom("Mine".into())
        );
        assert_eq!(
            settings.choice_for(Some("porklike")),
            PaletteChoice::Builtin(DmgTheme::Aqua)
        );
    }

    #[test]
    fn a_game_without_own_values_uses_all_games_and_stores_nothing() {
        let mut settings = Settings {
            matrix: true,
            ..Default::default()
        };
        let before = serde_json::to_value(&settings).expect("settings save");
        settings.play_game(Some("tobu"));
        assert!(settings.matrix);
        for scope in [Scope::AllGames, Scope::ThisGame] {
            settings.set_scope(scope);
            let frame = settings.begin_scope(shown());
            settings.end_scope(frame);
        }
        assert_eq!(settings.owned_rows(Surface::Desktop).count(), 0);
        assert_eq!(
            serde_json::to_value(&settings).expect("settings save"),
            before
        );
    }

    #[test]
    fn a_row_dropped_during_a_frame_goes_back_to_all_games_after_it() {
        let mut settings = Settings::default();
        settings.set_own("porklike", "matrix", on(true));
        settings.play_game(Some("porklike"));
        settings.set_scope(Scope::ThisGame);

        let frame = settings.begin_scope(shown());
        settings.drop_later(RowId::LcdGrid, Surface::Desktop);
        assert!(settings.matrix);
        settings.end_scope(frame);

        assert!(!settings.matrix);
        assert_eq!(settings.own_values("porklike").count(), 0);
    }

    #[test]
    fn values_that_stay_on_the_device_are_kept_apart_from_synced_ones() {
        let mut settings = Settings::default();
        settings.play_game(Some("pong"));
        settings.set_scope(Scope::ThisGame);
        let frame = settings.begin_scope(shown());
        settings.ds_jit = !settings.ds_jit;
        settings.ghosting = true;
        settings.end_scope(frame);

        let saved = serde_json::to_value(&settings).expect("settings save");
        assert_eq!(saved["per_game"]["pong"]["ghosting"], on(true));
        assert!(saved["per_game"]["pong"].get("ds_jit").is_none());
        assert!(saved["per_game_local"]["pong"].get("ds_jit").is_some());
    }

    #[test]
    fn leaving_the_game_scope_steps_back_to_all_games() {
        let mut settings = Settings::default();
        settings.set_scope(Scope::ThisGame);
        assert_eq!(settings.scope(), Scope::AllGames, "no game, no game scope");
        settings.play_game(Some("tobu"));
        settings.set_scope(Scope::ThisGame);
        assert!(settings.leave_scope());
        assert_eq!(settings.scope(), Scope::AllGames);
        assert!(!settings.leave_scope());
    }

    #[test]
    fn synced_settings_keep_the_running_games_values() {
        let mut settings = Settings::default();
        settings.set_own("porklike", "ghosting", on(true));
        settings.play_game(Some("porklike"));
        settings.set_scope(Scope::ThisGame);
        let mut values = super::super::synced::synced_values(&settings);
        values.insert("matrix".to_owned(), on(true));
        values.insert("ghosting".to_owned(), on(false));

        assert!(super::super::synced::adopt(&mut settings, &values));
        assert!(settings.matrix);
        assert!(settings.ghosting, "the game keeps its own value");
        assert_eq!(settings.scope(), Scope::ThisGame);
        settings.play_game(None);
        assert!(!settings.ghosting);
    }
}

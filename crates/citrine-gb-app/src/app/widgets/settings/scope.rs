use crate::app::mobile::sheet::percent;
use crate::app::ui_state::per_game::{Scope, ScopeFrame};
use crate::app::ui_state::settings::Settings;
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::ui_state::settings_tree::{GroupId, SettingsTab, Surface};
use crate::app::widgets::settings::palette::choice_label;
use crate::icons;
use citrine_gb::console::ConsoleKind;
use std::collections::BTreeMap;
use strum::IntoEnumIterator;

pub(crate) const ALL_GAMES: &str = "All games";
pub(crate) const THIS_GAME: &str = "This game";
pub(crate) const ONLY_HERE: &str = "Only for this game";
pub(crate) const CHANGED: &str = "Changed from all games";
pub(crate) const UNCHANGED: &str = "Same as all games";
pub(crate) const ALL_FOR_GAME: &str = "All settings for this game";
pub(crate) const USE_ALL: &str = "Use the value for all games";
pub(crate) const USE_ALL_HINT: &str = "Use all games";
pub(crate) const FOLLOWS: &str = "Follows all games. Changing it sets it for this game.";
pub(crate) const MARK: &str = icons::PUSH_PIN;

pub(crate) fn buttons<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    current: &mut T,
    first: (T, &str),
    last: (T, &str),
    whole: &str,
) {
    buttons_from(ui, current, first, last, whole, 0.0);
}

pub(crate) fn buttons_from<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    current: &mut T,
    first: (T, &str),
    last: (T, &str),
    whole: &str,
    first_width: f32,
) {
    ui.horizontal(|ui| {
        let button = egui::Button::selectable(*current == first.0, first.1)
            .min_size(egui::vec2(first_width, 0.0));
        if ui.add(button).clicked() {
            *current = first.0;
        }
        let full = egui::WidgetText::from(last.1)
            .into_galley(
                ui,
                Some(egui::TextWrapMode::Extend),
                f32::INFINITY,
                egui::TextStyle::Button,
            )
            .size()
            .x
            + 2.0 * ui.spacing().button_padding.x;
        let cut = full > ui.available_width();
        let button = egui::Button::selectable(*current == last.0, last.1).truncate();
        let mut response = ui.add(button);
        if cut {
            response = response.on_hover_text(whole);
        }
        if response.clicked() {
            *current = last.0;
        }
    });
}

pub(crate) fn changed_heading<T>(rows: &[T]) -> &'static str {
    if rows.is_empty() { UNCHANGED } else { CHANGED }
}

pub(crate) fn has_rows(tab: SettingsTab, surface: Surface, dev_mode: bool) -> bool {
    tab.groups_on(surface, dev_mode)
        .any(|group| group_has_rows(group, surface, dev_mode))
}

fn group_has_rows(group: GroupId, surface: Surface, dev_mode: bool) -> bool {
    group.rows_on(surface, dev_mode).any(RowId::per_game)
}

pub(crate) fn game_title(s: &Settings) -> &str {
    match s.playing_title().trim() {
        "" => THIS_GAME,
        title => title,
    }
}

pub(crate) fn first_tab(s: &Settings, surface: Surface) -> SettingsTab {
    let fits = |tab: SettingsTab| {
        fits_console(tab, s.playing_console()) && has_rows(tab, surface, s.dev_mode)
    };
    if fits(s.current_tab) {
        return s.current_tab;
    }
    SettingsTab::iter()
        .find(|tab| fits(*tab))
        .unwrap_or(s.current_tab)
}

fn fits_console(tab: SettingsTab, console: Option<ConsoleKind>) -> bool {
    match (tab, console) {
        (_, None) => true,
        (SettingsTab::GameBoy, Some(kind)) => matches!(kind, ConsoleKind::Gb | ConsoleKind::Gbc),
        (SettingsTab::Gba, Some(kind)) => kind == ConsoleKind::Gba,
        (SettingsTab::Ds, Some(kind)) => kind == ConsoleKind::Nds,
        _ => true,
    }
}

pub(crate) fn shows_bar(s: &Settings, tab: SettingsTab, surface: Surface) -> bool {
    s.playing_key().is_some()
        && fits_console(tab, s.playing_console())
        && has_rows(tab, surface, s.dev_mode)
}

#[derive(Clone, Copy)]
pub(crate) struct View {
    pub(crate) dev_mode: bool,
    pub(crate) bar: bool,
    pub(crate) game_only: bool,
    pub(crate) surface: Surface,
}

impl View {
    pub(crate) fn of(s: &Settings, tab: SettingsTab, surface: Surface) -> Self {
        let bar = shows_bar(s, tab, surface);
        Self {
            dev_mode: s.dev_mode,
            bar,
            game_only: bar && s.scope() == Scope::ThisGame,
            surface,
        }
    }

    pub(crate) fn row(self, row: RowId) -> bool {
        !self.game_only || row.per_game()
    }

    pub(crate) fn group(self, group: GroupId) -> bool {
        !self.game_only || group_has_rows(group, self.surface, self.dev_mode)
    }

    pub(crate) fn tag(self, s: &Settings, row: RowId) -> Option<Tag> {
        if !self.bar || !row.per_game() {
            return None;
        }
        let own = s.owns(row, self.surface);
        match s.scope() {
            Scope::ThisGame if own => Some(Tag::Own),
            Scope::ThisGame => Some(Tag::Follows),
            Scope::AllGames if own => s.game_text(row).map(|text| Tag::Differs(text.to_owned())),
            Scope::AllGames => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Tag {
    Own,
    Follows,
    Differs(String),
    Pick { game: String, value: String },
}

impl Tag {
    pub(crate) fn words(&self) -> String {
        match self {
            Self::Own => format!("{THIS_GAME} {}", icons::X),
            Self::Follows => ALL_GAMES.to_owned(),
            Self::Differs(_) => MARK.to_owned(),
            Self::Pick { game, value } => format!("{game}: {value} {}", icons::X),
        }
    }

    pub(crate) fn short_words(&self) -> String {
        match self {
            Self::Own => format!("{MARK} {}", icons::X),
            Self::Follows => String::new(),
            Self::Differs(_) | Self::Pick { .. } => self.words(),
        }
    }

    pub(crate) fn short_hover(&self) -> Option<String> {
        matches!(self, Self::Own).then(|| {
            format!(
                "This game's value. {} uses the value for all games",
                icons::X
            )
        })
    }

    pub(crate) fn hover(&self, title: &str) -> String {
        match self {
            Self::Own | Self::Pick { .. } => USE_ALL.to_owned(),
            Self::Follows => FOLLOWS.to_owned(),
            Self::Differs(value) => format!("{title} uses {value}"),
        }
    }

    pub(crate) fn own(&self) -> bool {
        !matches!(self, Self::Follows)
    }

    pub(crate) fn game_scope(&self) -> bool {
        matches!(self, Self::Own | Self::Follows)
    }

    pub(crate) fn drops(&self) -> bool {
        matches!(self, Self::Own | Self::Pick { .. })
    }
}

pub(crate) fn value_text(row: RowId, s: &Settings, surface: Surface) -> Option<String> {
    let on_off = |on: bool| if on { "On" } else { "Off" }.to_owned();
    let amount = |value: f32| percent(value.into());
    let screens = &s.ds_screens;
    Some(match row {
        RowId::Scaling => match surface {
            Surface::Desktop => s.scaling.to_string(),
            Surface::Phone => s.touch.scaling.to_string(),
        },
        RowId::SameSizePixels => on_off(s.same_size_pixels),
        RowId::Volume => amount(s.volume),
        RowId::SlowSound => s.slow_sound.to_string(),
        RowId::FastForwardSound => s.fast_forward_sound.to_string(),
        RowId::Rewind => s.rewind.to_string(),
        RowId::GbPulse1 => on_off(s.channels[0]),
        RowId::GbPulse2 => on_off(s.channels[1]),
        RowId::GbWave => on_off(s.channels[2]),
        RowId::GbNoise => on_off(s.channels[3]),
        RowId::GbaPulse1 => on_off(s.gba_channels[0]),
        RowId::GbaPulse2 => on_off(s.gba_channels[1]),
        RowId::GbaWave => on_off(s.gba_channels[2]),
        RowId::GbaNoise => on_off(s.gba_channels[3]),
        RowId::DirectSoundA => on_off(s.gba_channels[4]),
        RowId::DirectSoundB => on_off(s.gba_channels[5]),
        RowId::Sunlight => amount(s.gimmicks.light),
        RowId::TextureFilter => s.ds_3d.texture_filter.to_string(),
        RowId::DistantTextures => s.ds_3d.distant_textures.to_string(),
        RowId::Language => s.ds_user.language().to_string(),
        RowId::LcdGrid => on_off(s.matrix),
        RowId::Ghosting => on_off(s.ghosting),
        RowId::GridLines => amount(s.matrix_edge_darkness),
        RowId::GridCorners => amount(s.matrix_corner_darkness),
        RowId::GhostingStrength => amount(s.ghosting_strength),
        RowId::FastForward => s.fast_forward_speed.to_string(),
        RowId::RunGamesOn => s.console.to_string(),
        RowId::Palette => choice_label(&s.global_choice()),
        RowId::GbcScreen => s.color_correction.to_string(),
        RowId::GbaScreen => s
            .gba_color_correction
            .unwrap_or(s.color_correction)
            .to_string(),
        RowId::SmallCorner => screens.corner.to_string(),
        RowId::SmallSize => screens.small.to_string(),
        RowId::Gap => screens.gap_label(),
        RowId::Rotation => screens.rotation.to_string(),
        RowId::Resolution3d => s.ds_3d.resolution.to_string(),
        RowId::SmoothEdges => s.ds_3d.smooth_edges.to_string(),
        RowId::Jit => on_off(s.ds_jit),
        RowId::GbaJit => on_off(s.gba_jit()),
        _ => return None,
    })
}

pub(crate) fn begin(s: &mut Settings, tab: SettingsTab, surface: Surface) -> ScopeFrame {
    let shown: BTreeMap<RowId, String> = s
        .owned_rows(surface)
        .filter_map(|row| Some((row, value_text(row, s, surface)?)))
        .collect();
    let scope = if shows_bar(s, tab, surface) {
        s.scope()
    } else {
        Scope::AllGames
    };
    s.begin_scope_as(shown, scope)
}

pub(crate) fn changed_rows(s: &Settings, surface: Surface, dev_mode: bool) -> Vec<RowId> {
    s.owned_rows(surface)
        .filter(|row| row.shown_on(surface, dev_mode))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_for_another_console_edits_all_games_while_a_game_runs() {
        use crate::app::ui_state::settings::ConsoleChoice;
        let mut settings = Settings::default();
        settings.play_game(Some("tobu"));
        settings.name_console(Some(ConsoleKind::Nds));
        settings.set_scope(Scope::ThisGame);
        assert!(shows_bar(&settings, SettingsTab::Screen, Surface::Desktop));
        assert!(!shows_bar(
            &settings,
            SettingsTab::GameBoy,
            Surface::Desktop
        ));
        assert!(!shows_bar(&settings, SettingsTab::Gba, Surface::Phone));

        let frame = begin(&mut settings, SettingsTab::GameBoy, Surface::Desktop);
        assert!(matches!(frame, ScopeFrame::AllGames));
        settings.console = ConsoleChoice::GameBoyColor;
        settings.end_scope(frame);
        assert_eq!(settings.own_values("tobu").count(), 0);
        assert_eq!(settings.scope(), Scope::ThisGame);

        settings.current_tab = SettingsTab::GameBoy;
        assert_eq!(first_tab(&settings, Surface::Desktop), SettingsTab::Screen);
    }

    #[test]
    fn every_per_game_row_names_its_value() {
        let settings = Settings::default();
        for row in RowId::iter().filter(|row| row.per_game()) {
            for surface in Surface::iter() {
                assert!(
                    value_text(row, &settings, surface).is_some(),
                    "{row:?} on {surface:?}"
                );
            }
        }
    }

    fn other<T: IntoEnumIterator + PartialEq + serde::Serialize>(now: T) -> serde_json::Value {
        let next = T::iter()
            .find(|value| *value != now)
            .expect("a second value");
        serde_json::to_value(next).expect("a value")
    }

    fn own_values(d: &Settings) -> Vec<(String, serde_json::Value)> {
        use serde_json::json;
        let screens = &d.ds_screens;
        let channels = d.channels.map(|on| !on);
        let gba_channels = d.gba_channels.map(|on| !on);
        [
            ("scaling", other(d.scaling)),
            ("touch.scaling", other(d.touch.scaling)),
            ("same_size_pixels", json!(!d.same_size_pixels)),
            ("volume", json!(0.9)),
            ("slow_sound", other(d.slow_sound)),
            ("fast_forward_sound", other(d.fast_forward_sound)),
            ("rewind", other(d.rewind)),
            ("channels", json!(channels)),
            ("gba_channels", json!(gba_channels)),
            ("gimmicks.light", json!(0.9)),
            ("ds_3d.texture_filter", other(d.ds_3d.texture_filter)),
            ("ds_3d.distant_textures", other(d.ds_3d.distant_textures)),
            ("ds_user.language", other(d.ds_user.language())),
            ("matrix", json!(!d.matrix)),
            ("matrix_edge_darkness", json!(0.9)),
            ("matrix_corner_darkness", json!(0.9)),
            ("ghosting", json!(!d.ghosting)),
            ("ghosting_strength", json!(0.1)),
            ("fast_forward_speed", other(d.fast_forward_speed)),
            ("console", other(d.console)),
            ("dmg_theme", other(d.dmg_theme)),
            ("custom_palette", json!("Mine")),
            ("gbc_palette", json!(null)),
            ("color_correction", other(d.color_correction)),
            (
                "gba_color_correction",
                other(d.gba_color_correction.unwrap_or(d.color_correction)),
            ),
            ("ds_screens.corner", other(screens.corner)),
            ("ds_screens.small", other(screens.small)),
            ("ds_screens.gap", json!(screens.gap + 4)),
            ("ds_screens.rotation", other(screens.rotation)),
            ("ds_3d.resolution", other(d.ds_3d.resolution)),
            ("ds_3d.smooth_edges", other(d.ds_3d.smooth_edges)),
            ("ds_jit", json!(!d.ds_jit)),
            ("gba_jit", json!(!d.gba_jit())),
        ]
        .map(|(path, value)| (path.to_owned(), value))
        .into()
    }

    #[test]
    fn every_per_game_row_shows_the_games_value_while_it_runs() {
        use crate::app::ui_state::per_game::per_game_paths;
        let global = Settings::default();
        let values = own_values(&global);
        let mut listed: Vec<&str> = values.iter().map(|(path, _)| path.as_str()).collect();
        listed.sort_unstable();
        assert_eq!(
            listed,
            per_game_paths(),
            "give each per-game path a value that differs from the default"
        );

        for row in RowId::iter().filter(|row| row.per_game()) {
            for surface in Surface::iter() {
                let mut settings = Settings::default();
                for path in row.field().paths(surface) {
                    let (_, value) = values
                        .iter()
                        .find(|(listed, _)| listed == path)
                        .expect("a listed path");
                    settings.set_own("porklike", path, value.clone());
                }
                let all = value_text(row, &settings, surface);
                settings.play_game(Some("porklike"));
                let game = value_text(row, &settings, surface);
                assert_ne!(game, all, "{row:?} on {surface:?} ignores the game's value");
                assert!(settings.owns(row, surface), "{row:?} on {surface:?}");
                settings.play_game(None);
                assert_eq!(
                    value_text(row, &settings, surface),
                    all,
                    "{row:?} on {surface:?} keeps the game's value after it"
                );
            }
        }
    }
}

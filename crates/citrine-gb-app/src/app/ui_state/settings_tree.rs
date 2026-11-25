#![cfg_attr(not(test), allow(dead_code))]

pub mod rows;
pub mod search;

use crate::app::ui_state::settings::kind_icon;
use crate::icons;
use citrine_gb::console::ConsoleKind;
use rows::RowId;
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, EnumIter)]
pub enum Surface {
    Desktop,
    Phone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, EnumIter)]
pub enum Section {
    AllConsoles,
    Consoles,
    Citrine,
}

impl Section {
    pub fn title(self) -> &'static str {
        match self {
            Self::AllConsoles => "All consoles",
            Self::Consoles => "Consoles",
            Self::Citrine => "Citrine",
        }
    }

    pub fn pages(self) -> &'static [SettingsTab] {
        match self {
            Self::AllConsoles => &[
                SettingsTab::Screen,
                SettingsTab::Sound,
                SettingsTab::Controls,
                SettingsTab::Playing,
                SettingsTab::Capture,
            ],
            Self::Consoles => &[SettingsTab::GameBoy, SettingsTab::Gba, SettingsTab::Ds],
            Self::Citrine => &[
                SettingsTab::App,
                SettingsTab::Online,
                SettingsTab::Sync,
                SettingsTab::Storage,
                SettingsTab::About,
                SettingsTab::Developer,
            ],
        }
    }
}

#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    EnumIter,
    serde::Serialize,
    serde::Deserialize,
)]
pub enum SettingsTab {
    #[default]
    #[serde(alias = "Style")]
    Screen,
    Sound,
    Controls,
    Playing,
    Capture,
    GameBoy,
    Gba,
    Ds,
    #[serde(alias = "General")]
    App,
    Online,
    Sync,
    #[serde(alias = "YourData")]
    Storage,
    About,
    Developer,
}

impl SettingsTab {
    pub fn title(self) -> &'static str {
        match self {
            Self::Screen => "Screen",
            Self::Sound => "Sound",
            Self::Controls => "Controls",
            Self::Playing => "Playing",
            Self::Capture => "Capture",
            Self::GameBoy => "Game Boy",
            Self::Gba => "Game Boy Advance",
            Self::Ds => "Nintendo DS",
            Self::App => "App",
            Self::Online => "Online",
            Self::Sync => "Sync",
            Self::Storage => "Storage",
            Self::About => "About",
            Self::Developer => "Developer",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Screen => icons::MONITOR,
            Self::Sound => icons::SPEAKER_HIGH,
            Self::Controls => icons::JOYSTICK,
            Self::Playing => icons::PLAY,
            Self::Capture => icons::CAMERA,
            Self::GameBoy => kind_icon(ConsoleKind::Gb),
            Self::Gba => kind_icon(ConsoleKind::Gba),
            Self::Ds => kind_icon(ConsoleKind::Nds),
            Self::App => icons::APP_WINDOW,
            Self::Online => icons::GLOBE_SIMPLE,
            Self::Sync => icons::ARROWS_CLOCKWISE,
            Self::Storage => icons::CHART_PIE_SLICE,
            Self::About => icons::INFO,
            Self::Developer => icons::BRACKETS_CURLY,
        }
    }

    pub fn section(self) -> Section {
        match self {
            Self::Screen | Self::Sound | Self::Controls | Self::Playing | Self::Capture => {
                Section::AllConsoles
            }
            Self::GameBoy | Self::Gba | Self::Ds => Section::Consoles,
            Self::App
            | Self::Online
            | Self::Sync
            | Self::Storage
            | Self::About
            | Self::Developer => Section::Citrine,
        }
    }

    pub fn groups(self) -> &'static [GroupId] {
        use GroupId::*;
        match self {
            Self::Screen => &[Size, Effects, Huds],
            Self::Sound => &[Output, Timing],
            Self::Controls => &[Keys, Controller, Split, Touch, BigScreen],
            Self::Playing => &[Speed, Saves, Pause],
            Self::Capture => &[Capture],
            Self::GameBoy => &[GbSystem, GbColours, GbChannels],
            Self::Gba => &[GbaSystem, GbaColours, GbaChannels, GbaPerformance, Solar],
            Self::Ds => &[
                DsScreens,
                Ds3d,
                DsUser,
                DsMicrophone,
                DsPerformance,
                DsSystem,
                Walker,
            ],
            Self::App => &[Look, Library, Achievements, Discord, Updates],
            Self::Online => &[
                Account,
                Friends,
                Notifications,
                PlayTogether,
                Devices,
                Storage,
                Leave,
            ],
            Self::Sync => &[Cloud, Network, Elsewhere],
            Self::Storage => &[YourData, Cache],
            Self::About => &[About],
            Self::Developer => &[Developer],
        }
    }

    pub fn available(self, dev_mode: bool) -> bool {
        match self {
            Self::Gba => cfg!(feature = "gba"),
            Self::Ds => cfg!(feature = "nds"),
            Self::Developer => dev_mode,
            _ => true,
        }
    }

    pub fn rows(self) -> impl Iterator<Item = RowId> {
        self.groups().iter().flat_map(|group| group.rows())
    }

    pub fn iter_with_dev_mode(dev_mode: bool) -> impl Iterator<Item = SettingsTab> {
        SettingsTab::iter().filter(move |page| page.available(dev_mode))
    }

    pub fn groups_on(self, surface: Surface, dev_mode: bool) -> impl Iterator<Item = GroupId> {
        self.groups()
            .iter()
            .copied()
            .filter(move |group| group.rows_on(surface, dev_mode).next().is_some())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, EnumIter)]
pub enum GroupId {
    Size,
    Effects,
    Huds,
    Output,
    Timing,
    Keys,
    Controller,
    Split,
    Touch,
    BigScreen,
    Speed,
    Saves,
    Pause,
    Capture,
    GbSystem,
    GbColours,
    GbChannels,
    GbaSystem,
    GbaColours,
    GbaChannels,
    GbaPerformance,
    Solar,
    DsScreens,
    Ds3d,
    DsUser,
    DsMicrophone,
    DsPerformance,
    DsSystem,
    Walker,
    Look,
    Library,
    Achievements,
    Discord,
    Updates,
    Account,
    Friends,
    Notifications,
    PlayTogether,
    Devices,
    Storage,
    Leave,
    Cloud,
    Network,
    Elsewhere,
    YourData,
    Cache,
    About,
    Developer,
}

impl GroupId {
    pub fn title(self) -> &'static str {
        match self {
            Self::Size => "Size",
            Self::Effects => "Screen effects",
            Self::Huds => "HUDs",
            Self::Output => "Output",
            Self::Timing => "Timing",
            Self::Keys => "Keys and buttons",
            Self::Controller => "Controller",
            Self::Split => "Two players, one keyboard",
            Self::Touch => "On-screen buttons",
            Self::BigScreen => "Big screen",
            Self::Speed => "Speed",
            Self::Saves => "Saves",
            Self::Pause => "Pause",
            Self::GbSystem | Self::GbaSystem | Self::DsSystem => "System",
            Self::GbColours | Self::GbaColours => "Colours",
            Self::GbChannels | Self::GbaChannels => "Channels",
            Self::Solar => "Solar sensor",
            Self::DsScreens => "Screens",
            Self::Ds3d => "3D",
            Self::DsUser => "DS user",
            Self::DsMicrophone => "Microphone",
            Self::DsPerformance | Self::GbaPerformance => "Performance",
            Self::Walker => "Pokéwalker",
            Self::Look => "Look",
            Self::Library => "Library",
            Self::Achievements => "Achievements",
            Self::Discord => "Discord",
            Self::Updates => "Updates",
            Self::Account => "Account",
            Self::Friends => "Friends",
            Self::Notifications => "Notifications",
            Self::PlayTogether => "Play together",
            Self::Cloud => "Cloud",
            Self::Network => "This network",
            Self::Elsewhere => "Saves elsewhere",
            Self::Devices => "Devices",
            Self::Storage => "Cloud storage",
            Self::Leave => "Leave or delete",
            Self::YourData => "Your data",
            Self::Cache => "Cache",
            Self::Capture | Self::About | Self::Developer => "",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Size => icons::MONITOR,
            Self::Effects => icons::GRID_NINE,
            Self::Huds => icons::GAUGE,
            Self::Output => icons::HEADPHONES,
            Self::Timing => icons::TIMER,
            Self::Keys => icons::KEYBOARD,
            Self::Controller => icons::GAME_CONTROLLER,
            Self::Split => icons::SQUARE_SPLIT_HORIZONTAL,
            Self::Touch => icons::HAND_TAP,
            Self::BigScreen => icons::TELEVISION_SIMPLE,
            Self::Speed => icons::FAST_FORWARD,
            Self::Saves => icons::SAVE_STATE,
            Self::Pause => icons::PAUSE,
            Self::GbSystem | Self::GbaSystem | Self::DsSystem => icons::CPU,
            Self::GbColours | Self::GbaColours => icons::PALETTE,
            Self::GbChannels | Self::GbaChannels => icons::WAVEFORM,
            Self::Solar => icons::SUN,
            Self::DsScreens => icons::SQUARE_SPLIT_VERTICAL,
            Self::Ds3d => icons::CUBE,
            Self::DsUser => icons::IDENTIFICATION_CARD,
            Self::DsMicrophone => icons::MICROPHONE,
            Self::DsPerformance | Self::GbaPerformance => icons::LIGHTNING,
            Self::Walker => icons::POKEWALKER,
            Self::Look => icons::PAINT_BRUSH_HOUSEHOLD,
            Self::Library => icons::BOOKS,
            Self::Achievements => icons::TROPHY,
            Self::Discord => icons::DISCORD_LOGO,
            Self::Updates => icons::ARROW_CIRCLE_UP,
            Self::Account => icons::USER,
            Self::Friends => icons::USERS,
            Self::Notifications => icons::BELL,
            Self::PlayTogether => icons::USERS,
            Self::Cloud => icons::CLOUD,
            Self::Network => icons::BROADCAST,
            Self::Elsewhere => icons::CLOUD_ARROW_DOWN,
            Self::Devices => icons::DEVICES,
            Self::Storage => icons::HARD_DRIVES,
            Self::Leave => icons::WARNING,
            Self::YourData => icons::FOLDER_USER,
            Self::Cache => icons::FOLDER_DASHED,
            Self::Capture | Self::About | Self::Developer => self.page().icon(),
        }
    }

    pub fn page(self) -> SettingsTab {
        SettingsTab::iter()
            .find(|page| page.groups().contains(&self))
            .unwrap_or(SettingsTab::App)
    }

    pub fn rows(self) -> impl Iterator<Item = RowId> {
        RowId::iter().filter(move |row| row.group() == self)
    }

    pub fn available(self, dev_mode: bool) -> bool {
        self.rows().any(|row| row.available(dev_mode))
    }

    pub fn rows_on(self, surface: Surface, dev_mode: bool) -> impl Iterator<Item = RowId> {
        self.rows()
            .filter(move |row| row.shown_on(surface, dev_mode))
    }
}

#[cfg(test)]
mod tests {
    use super::rows::{Field, ThisGameRow};
    use super::search::search;
    use super::*;
    use crate::app::ui_state::synced::setting_paths;

    const NO_ROW: &[(&str, &str)] = &[
        (
            "ui_scale_per_ratio",
            "internal: marks the one-time UI scale move",
        ),
        ("game_names", "internal: names learned from loaded games"),
        ("current_tab", "internal: the open Settings page"),
        ("quick_slot", "internal: the last quick save slot"),
        ("record_frame_times", "set in the Performance pane"),
        ("focus_place", "internal: where focus mode put the screen"),
        ("synced", "internal: sync bookkeeping"),
        ("per_game", "internal: each game's own values"),
        (
            "per_game_local",
            "internal: each game's own values that stay on this device",
        ),
        (
            "online.notify_asked",
            "internal: the notification prompt was shown",
        ),
        (
            "big_screen.menu_opened",
            "internal: the game menu was opened, which ends its tip",
        ),
        ("stats_range", "set in the Stats tab"),
        ("track_pc", "set in the Disassembly tab"),
        ("hud.huds", "set from the HUD menu"),
        ("hud.tracker", "placed by dragging it or in Edit layout"),
        ("layouts", "set in Edit layout from the Layout menu"),
        ("layout_picks", "picked in the Layout menu"),
        ("saved_layouts", "made in Edit layout from the Layout menu"),
        (
            "layouts_adopted",
            "internal: marks the one-time move of old layouts into saved ones",
        ),
        (
            "touch.opacity_rechecked",
            "internal: marks the one-time move off the old button opacity",
        ),
        (
            "clip_length_adopted",
            "internal: marks the one-time copy of the rewind length into the clip length",
        ),
        (
            "ds_screens.desktop",
            "the default before a layout is picked in the Layout menu",
        ),
        (
            "ds_screens.portrait",
            "the default before a layout is picked in the Layout menu",
        ),
        (
            "ds_screens.landscape",
            "the default before a layout is picked in the Layout menu",
        ),
        ("achievements.account", "set in the Achievements tab"),
        ("achievements.hardcore", "set in the Achievements tab"),
        ("local_link.controls", "set in the Linked tab"),
        ("local_link.swapped", "set in the Linked tab"),
        ("local_link.second_sound", "set in the Linked tab"),
        (
            "online.friend_volume",
            "set in the play together sound menu",
        ),
        ("online.friend_muted", "set in the play together sound menu"),
    ];

    const ONE_SURFACE: &[(&str, Surface, &str)] = &[
        (
            "ui_scale",
            Surface::Desktop,
            "the touch layout has touch.ui_scale",
        ),
        (
            "touch.ui_scale",
            Surface::Phone,
            "the desktop layout has ui_scale",
        ),
        (
            "scaling",
            Surface::Desktop,
            "the touch layout has touch.scaling",
        ),
        (
            "ds_screens.separate_tabs",
            Surface::Desktop,
            "only the desktop layout has tabs",
        ),
        (
            "online.show_playing",
            Surface::Desktop,
            "the phone has it on the You place",
        ),
        (
            "online.relays",
            Surface::Desktop,
            "the phone switches accounts on the You place",
        ),
        (
            "online.joined",
            Surface::Desktop,
            "the phone signs out on the You place",
        ),
        (
            "online.device_name",
            Surface::Desktop,
            "the phone lists devices on the You place",
        ),
        (
            "online.focus_popups",
            Surface::Desktop,
            "only the desktop layout has focus mode popups",
        ),
        (
            "capture_folder",
            Surface::Desktop,
            "only a desktop build picks folders",
        ),
        (
            "talk_button",
            Surface::Phone,
            "only the touch layout draws the talk button",
        ),
    ];

    const LEANS_ON_ITS_NEIGHBOURS: &[&str] = &[
        "Every",
        "Keep",
        "Speed",
        "How far back",
        "Strength",
        "Line darkness",
        "Crossing darkness",
        "Slowdown",
        "Layout",
        "Console",
        "Status",
        "Name",
        "Code",
    ];

    fn rows_on(page: SettingsTab, surface: Surface) -> impl Iterator<Item = RowId> {
        page.rows()
            .filter(move |row| row.surfaces().contains(&surface))
    }

    fn found(query: &str, surface: Surface, dev_mode: bool) -> Vec<RowId> {
        search(query, surface, dev_mode)
            .into_iter()
            .map(|hit| hit.row)
            .collect()
    }

    #[test]
    fn every_page_has_groups_and_every_group_rows() {
        let mut pages = Vec::new();
        for section in Section::iter() {
            assert!(!section.title().is_empty());
            for page in section.pages() {
                assert_eq!(page.section(), section, "{page:?}");
                pages.push(*page);
            }
        }
        assert_eq!(pages, SettingsTab::iter().collect::<Vec<_>>());

        let mut groups = Vec::new();
        for page in SettingsTab::iter() {
            assert!(!page.groups().is_empty(), "{page:?} has no groups");
            for group in page.groups() {
                assert_eq!(group.page(), page, "{group:?}");
                assert!(group.rows().next().is_some(), "{group:?} has no rows");
                groups.push(*group);
            }
            for surface in Surface::iter() {
                assert!(
                    rows_on(page, surface).next().is_some(),
                    "{page:?} is empty on {surface:?}"
                );
            }
        }
        assert_eq!(groups, GroupId::iter().collect::<Vec<_>>());

        for page in SettingsTab::iter() {
            assert!(!page.icon().is_empty(), "{page:?}");
            assert_eq!(
                page.available(true),
                page.groups().iter().any(|group| group.available(true)),
                "{page:?}"
            );
        }
        for group in GroupId::iter() {
            assert!(!group.icon().is_empty(), "{group:?}");
        }
        for row in RowId::iter() {
            assert!(!row.surfaces().is_empty(), "{row:?} shows nowhere");
            for surface in row.surfaces().iter().copied() {
                let names_a_setting = matches!(
                    row.field(),
                    Field::One(_) | Field::Many(_) | Field::PerSurface { .. }
                );
                assert_eq!(
                    !row.field().paths(surface).is_empty(),
                    names_a_setting,
                    "{row:?} on {surface:?}"
                );
            }
        }
        for row in ThisGameRow::iter() {
            assert!(!row.label().is_empty());
            assert!(row.field().paths(Surface::Phone).len() <= 1, "{row:?}");
        }
    }

    #[test]
    fn row_labels_are_unique_on_each_page() {
        for page in SettingsTab::iter() {
            for surface in Surface::iter() {
                let mut labels: Vec<&str> = rows_on(page, surface)
                    .map(|row| row.label(surface))
                    .collect();
                let count = labels.len();
                labels.sort_unstable();
                labels.dedup();
                assert_eq!(labels.len(), count, "{page:?} on {surface:?}: {labels:?}");
            }
            let mut titles: Vec<&str> = page
                .groups()
                .iter()
                .map(|group| group.title())
                .filter(|title| !title.is_empty())
                .collect();
            let count = titles.len();
            titles.sort_unstable();
            titles.dedup();
            assert_eq!(titles.len(), count, "{page:?} group titles");
        }
        let mut this_game: Vec<&str> = ThisGameRow::iter().map(|row| row.label()).collect();
        let count = this_game.len();
        this_game.sort_unstable();
        this_game.dedup();
        assert_eq!(this_game.len(), count);
    }

    fn reads_alone(label: &str) -> Result<(), &'static str> {
        let first = label.chars().next().ok_or("empty")?;
        if !(first.is_uppercase() || first.is_ascii_digit()) {
            return Err("starts in lower case");
        }
        if label.trim() != label || label.contains("  ") {
            return Err("stray spaces");
        }
        if label.ends_with('.') {
            return Err("ends in a full stop");
        }
        if label.chars().any(|c| {
            matches!(
                c,
                ':' | ';' | '(' | ')' | '/' | '\u{2026}' | '\u{2014}' | '\u{2013}'
            )
        }) {
            return Err("punctuation that needs its neighbours");
        }
        if label
            .split(' ')
            .any(|word| word == "New" || word == "Later")
        {
            return Err("badge text");
        }
        if LEANS_ON_ITS_NEIGHBOURS.contains(&label) {
            return Err("only reads beside its neighbours");
        }
        Ok(())
    }

    #[test]
    fn every_label_reads_alone_in_search() {
        for row in RowId::iter() {
            for surface in row.surfaces().iter().copied() {
                let label = row.label(surface);
                if let Err(why) = reads_alone(label) {
                    panic!("{row:?} on {surface:?}: {label:?} {why}");
                }
                for keyword in row.keywords() {
                    assert_eq!(*keyword, keyword.to_lowercase(), "{row:?} keyword");
                    assert!(
                        !keyword
                            .split(' ')
                            .any(|word| word == "new" || word == "later"),
                        "{row:?} keyword {keyword:?}"
                    );
                }
                if !row.available(true) {
                    continue;
                }
                let hits = search(label, surface, true);
                assert!(
                    hits.iter().any(|hit| hit.row == row),
                    "{row:?}: searching {label:?} on {surface:?} misses it"
                );
                assert!(
                    hits[0].row.label(surface).starts_with(label),
                    "{row:?}: searching {label:?} on {surface:?} puts {:?} first",
                    hits[0].row
                );
            }
        }
        for row in ThisGameRow::iter() {
            if let Err(why) = reads_alone(row.label()) {
                panic!("{row:?}: {why}");
            }
        }
    }

    #[cfg(feature = "nds")]
    #[test]
    fn search_finds_the_microphone_rows_on_the_ds_page() {
        let mic = [RowId::Microphone, RowId::PushToTalk, RowId::TalkButton];
        for surface in Surface::iter() {
            let button = surface == Surface::Phone;
            for query in ["mic", "microphone"] {
                let hits = found(query, surface, false);
                assert_eq!(hits.first(), Some(&RowId::Microphone), "{query}");
                assert!(hits.contains(&RowId::PushToTalk), "{query}: {hits:?}");
                assert_eq!(
                    hits.contains(&RowId::TalkButton),
                    button,
                    "{query}: {hits:?}"
                );
            }
            let talk = found("push to talk", surface, false);
            assert_eq!(talk.first(), Some(&RowId::PushToTalk), "{talk:?}");
            assert_eq!(talk.contains(&RowId::TalkButton), button, "{talk:?}");
        }
        for row in mic {
            assert_eq!(row.page(), SettingsTab::Ds);
        }
    }

    #[cfg(feature = "nds")]
    #[test]
    fn search_finds_each_console_jit_switch() {
        if !citrine_gb::nds::machine::JIT_AVAILABLE {
            return;
        }
        for surface in Surface::iter() {
            for query in ["jit", "faster", "dynarec"] {
                let hits = found(query, surface, false);
                assert!(hits.contains(&RowId::Jit), "{query}: {hits:?}");
                assert!(hits.contains(&RowId::GbaJit), "{query}: {hits:?}");
            }
            assert_eq!(found("gba jit", surface, false)[0], RowId::GbaJit);
            assert_eq!(found("ds jit", surface, false)[0], RowId::Jit);
        }
        assert_eq!(RowId::GbaJit.page(), SettingsTab::Gba);
        assert_eq!(RowId::GbaCompiledCode.page(), SettingsTab::Gba);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_ds_3d_rows_go_by_their_filter_names_and_keep_the_old_words() {
        for surface in Surface::iter() {
            assert_eq!(
                RowId::DistantTextures.label(surface),
                "Anisotropic filtering"
            );
            for (query, row) in [
                ("anisotropic", RowId::DistantTextures),
                ("shimmer", RowId::DistantTextures),
                ("fix distant shimmer", RowId::DistantTextures),
                ("sharpest", RowId::DistantTextures),
                ("trilinear", RowId::DistantTextures),
                ("texture filter", RowId::TextureFilter),
                ("pixelated", RowId::TextureFilter),
                ("rounded", RowId::TextureFilter),
                ("bicubic", RowId::TextureFilter),
            ] {
                let hits = found(query, surface, false);
                assert!(hits.contains(&row), "{query}: {hits:?}");
            }
        }
    }

    #[test]
    fn search_finds_a_row_by_label_keyword_and_group() {
        let desktop = Surface::Desktop;
        assert_eq!(
            found("autosave every", desktop, false)[0],
            RowId::AutosaveEvery
        );
        assert_eq!(
            found("AUTOSAVE EV", desktop, false)[0],
            RowId::AutosaveEvery
        );
        assert_eq!(found("refresh", desktop, false)[0], RowId::MatchRefresh);
        assert_eq!(found("vsync", desktop, false), [RowId::MatchRefresh]);

        let effects: Vec<RowId> = GroupId::Effects.rows().collect();
        assert_eq!(found("screen effects", desktop, false), effects);

        if cfg!(feature = "nds") {
            let walker = found("pokewalker", desktop, false);
            assert!(walker.contains(&RowId::StepsInGame), "{walker:?}");
        }

        let pulse = found("pulse 1", desktop, false);
        assert!(pulse.contains(&RowId::GbPulse1), "{pulse:?}");
        if cfg!(feature = "gba") {
            assert!(pulse.contains(&RowId::GbaPulse1), "{pulse:?}");
        }

        let playing = found("playing", desktop, false);
        assert!(playing.contains(&RowId::FastForward), "{playing:?}");

        let screen = found("screen", desktop, false);
        let at = |row: RowId| screen.iter().position(|r| *r == row).unwrap();
        assert!(at(RowId::ScreenshotSize) < at(RowId::LcdGrid), "{screen:?}");

        assert!(found("", desktop, true).is_empty());
        assert!(found("   ", desktop, true).is_empty());
        assert!(found("zzzz", desktop, true).is_empty());
    }

    #[test]
    fn search_finds_the_clip_length_by_the_words_people_use() {
        for surface in Surface::iter() {
            for query in ["clip", "save a clip", "instant replay", "how far back"] {
                let hits = found(query, surface, false);
                assert!(
                    hits.contains(&RowId::ClipLength),
                    "{query:?} on {surface:?}: {hits:?}"
                );
            }
            assert_eq!(found("clip", surface, false)[0], RowId::ClipLength);
            assert_eq!(RowId::ClipLength.page(), SettingsTab::Capture);
        }
    }

    #[test]
    fn search_hides_developer_rows_outside_dev_mode() {
        for surface in Surface::iter() {
            assert!(found("randomized", surface, false).is_empty());
            assert!(found("randomized", surface, true).contains(&RowId::RandomRam));
            for row in RowId::iter() {
                let hits = search(row.label(surface), surface, false);
                assert!(
                    hits.iter().all(|hit| hit.page != SettingsTab::Developer),
                    "{row:?} on {surface:?}"
                );
            }
        }
        assert!(found("dev tools", Surface::Desktop, false).contains(&RowId::DevTools));
    }

    #[test]
    fn your_data_is_a_section_of_storage_and_every_search_for_it_lands_there() {
        assert_eq!(
            Section::Citrine.pages(),
            [
                SettingsTab::App,
                SettingsTab::Online,
                SettingsTab::Sync,
                SettingsTab::Storage,
                SettingsTab::About,
                SettingsTab::Developer,
            ]
        );
        assert_eq!(
            SettingsTab::Storage.groups(),
            [GroupId::YourData, GroupId::Cache]
        );
        assert_eq!(GroupId::YourData.title(), "Your data");
        assert_eq!(
            GroupId::YourData.title(),
            crate::storage::kinds::Section::Data.title()
        );
        assert_eq!(
            GroupId::YourData.rows().collect::<Vec<_>>(),
            [
                RowId::SpaceUsed,
                RowId::Export,
                RowId::Import,
                RowId::DeleteEverything,
            ]
        );
        for surface in Surface::iter() {
            let hits = search("your data", surface, false);
            assert!(!hits.is_empty(), "{surface:?}");
            assert!(
                hits.iter()
                    .all(|hit| hit.page == SettingsTab::Storage && hit.group == GroupId::YourData),
                "{surface:?}: {hits:?}"
            );
            for (query, row) in [
                ("export", RowId::Export),
                ("import", RowId::Import),
                ("delete", RowId::DeleteEverything),
                ("backup", RowId::Export),
            ] {
                let hits = search(query, surface, false);
                let hit = hits
                    .iter()
                    .find(|hit| hit.row == row)
                    .unwrap_or_else(|| panic!("{query} on {surface:?} misses {row:?}: {hits:?}"));
                assert_eq!(hit.page, SettingsTab::Storage, "{query} on {surface:?}");
                assert_eq!(hit.group, GroupId::YourData, "{query} on {surface:?}");
            }
        }
    }

    #[test]
    fn search_skips_rows_a_surface_lacks() {
        assert!(!found("menu button", Surface::Desktop, true).contains(&RowId::MenuButton));
        assert!(found("menu button", Surface::Phone, true).contains(&RowId::MenuButton));
        assert!(!found("game scaling", Surface::Phone, true).contains(&RowId::TouchScaling));
        assert!(found("game scaling", Surface::Desktop, true).contains(&RowId::TouchScaling));
        for row in RowId::iter() {
            for surface in Surface::iter() {
                if row.surfaces().contains(&surface) {
                    continue;
                }
                let label = row.label(Surface::Desktop);
                assert!(
                    !found(label, surface, true).contains(&row),
                    "{row:?} found on {surface:?}"
                );
            }
        }
        assert_eq!(
            found("count phone steps", Surface::Phone, true).contains(&RowId::PhoneSteps),
            cfg!(all(feature = "nds", target_os = "android"))
        );
        assert!(matches!(RowId::Volume.field(), Field::One("volume")));
    }

    fn covers(field: &str, path: &str) -> bool {
        field == path
            || path.starts_with(&format!("{field}."))
            || field.starts_with(&format!("{path}."))
    }

    fn row_on(path: &str, surface: Surface) -> bool {
        RowId::iter().any(|row| {
            row.surfaces().contains(&surface)
                && row
                    .field()
                    .paths(surface)
                    .iter()
                    .any(|field| covers(field, path))
        })
    }

    fn in_this_game(path: &str) -> bool {
        ThisGameRow::iter().any(|row| {
            row.field()
                .paths(Surface::Desktop)
                .iter()
                .any(|field| covers(field, path))
        })
    }

    #[test]
    fn every_setting_has_a_row_on_both_surfaces_or_a_reason() {
        let paths = setting_paths();
        let is_setting = |field: &str| paths.iter().any(|path| covers(field, path));
        assert!(paths.iter().any(|path| path == "volume"), "{paths:?}");

        let mut missing = Vec::new();
        for path in &paths {
            if in_this_game(path) || NO_ROW.iter().any(|(p, _)| covers(p, path)) {
                continue;
            }
            for surface in Surface::iter() {
                let elsewhere = ONE_SURFACE
                    .iter()
                    .any(|(p, only, _)| *only != surface && covers(p, path));
                if !elsewhere && !row_on(path, surface) {
                    missing.push(format!("{path} on {surface:?}"));
                }
            }
        }
        assert!(
            missing.is_empty(),
            "give these a tree row, or a reason in NO_ROW or ONE_SURFACE: {missing:#?}"
        );

        for &(path, why) in NO_ROW {
            assert!(
                !why.is_empty() && is_setting(path),
                "{path} is not a setting"
            );
            for surface in Surface::iter() {
                assert!(!row_on(path, surface), "{path} has a row on {surface:?}");
            }
        }
        for &(path, only, why) in ONE_SURFACE {
            assert!(
                !why.is_empty() && is_setting(path),
                "{path} is not a setting"
            );
            for surface in Surface::iter() {
                assert_eq!(
                    row_on(path, surface),
                    surface == only,
                    "{path} on {surface:?}"
                );
            }
        }
        for row in ThisGameRow::iter() {
            for field in row.field().paths(Surface::Desktop) {
                assert!(is_setting(field), "{row:?}: {field} is not a setting");
            }
        }
        for row in RowId::iter() {
            for surface in row.surfaces().iter().copied() {
                for field in row.field().paths(surface) {
                    assert!(is_setting(field), "{row:?}: {field} is not a setting");
                }
            }
            if let Field::Pending(field) = row.field() {
                assert!(
                    !is_setting(field),
                    "{row:?}: {field} exists now, so make it Field::One"
                );
            }
        }
    }

    #[test]
    fn one_label_one_field_per_page() {
        for surface in Surface::iter() {
            let on: Vec<(RowId, Vec<&str>)> = RowId::iter()
                .filter(|row| row.surfaces().contains(&surface))
                .map(|row| (row, row.field().paths(surface)))
                .filter(|(_, paths)| !paths.is_empty())
                .collect();
            let shared =
                |path: &str| on.iter().filter(|(_, paths)| paths.contains(&path)).count() > 1;
            let mut seen: Vec<(SettingsTab, String, &Vec<&str>, RowId)> = Vec::new();
            for (row, paths) in &on {
                if paths.iter().any(|&path| shared(path)) {
                    continue;
                }
                let label = row.label(surface).to_lowercase();
                if let Some((_, _, other, first)) = seen
                    .iter()
                    .find(|(page, l, _, _)| *page == row.page() && *l == label)
                {
                    assert_eq!(
                        *other, paths,
                        "{first:?} and {row:?} share a label on {surface:?}"
                    );
                }
                seen.push((row.page(), label, paths, *row));
            }
        }
    }

    fn sources(path: &std::path::Path, found: &mut Vec<(std::path::PathBuf, String)>) {
        if path.is_dir() {
            let mut entries: Vec<_> = std::fs::read_dir(path)
                .expect("a source folder")
                .map(|entry| entry.expect("an entry").path())
                .collect();
            entries.sort();
            for entry in entries {
                sources(&entry, found);
            }
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let text = std::fs::read_to_string(path).expect("a source file");
            let shipped = text.split("#[cfg(test)]").next().unwrap_or_default();
            found.push((path.to_owned(), shipped.to_owned()));
        }
    }

    #[test]
    fn no_badge_text_in_settings() {
        let app = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/app");
        let mut found = Vec::new();
        for part in [
            "widgets/settings",
            "widgets/settings.rs",
            "mobile/settings_pages",
            "mobile/settings_pages.rs",
            "mobile/settings_list.rs",
            "tabs/settings.rs",
            "ui_state/settings_tree",
            "ui_state/settings_tree.rs",
        ] {
            sources(&app.join(part), &mut found);
        }
        assert!(found.len() > 10, "{}", found.len());
        for (path, text) in &found {
            for badge in ["\"New\"", "\"Later\""] {
                assert!(!text.contains(badge), "{badge} in {}", path.display());
            }
        }
    }

    const GAME_PAGES: [SettingsTab; 6] = [
        SettingsTab::Screen,
        SettingsTab::Sound,
        SettingsTab::Playing,
        SettingsTab::GameBoy,
        SettingsTab::Gba,
        SettingsTab::Ds,
    ];

    const STAYS_GLOBAL: &[RowId] = &[
        RowId::MatchRefresh,
        RowId::ShowHuds,
        RowId::HudOpacity,
        RowId::Mute,
        RowId::OutputDevice,
        RowId::Latency,
        RowId::FastForwardKey,
        RowId::AutosaveEvery,
        RowId::AutosavesKept,
        RowId::QuickLoad,
        RowId::InBackground,
        RowId::CameraSees,
        RowId::OwnPalettes,
        RowId::LightSensor,
        RowId::TwoTabs,
        RowId::Nickname,
        RowId::Birthday,
        RowId::Colour,
        RowId::Clock,
        RowId::KeepFirmware,
        RowId::OnlineService,
        RowId::DsDirect,
        RowId::Dns,
        RowId::StepsInGame,
        RowId::PhoneSteps,
        RowId::Microphone,
        RowId::PushToTalk,
        RowId::TalkButton,
    ];

    #[test]
    fn every_row_that_changes_a_game_can_be_pinned_to_it() {
        let names_a_setting = |row: &RowId| {
            !matches!(
                row.field(),
                Field::Action | Field::Server | Field::Pending(_)
            )
        };
        let global: Vec<RowId> = RowId::iter()
            .filter(|row| GAME_PAGES.contains(&row.page()))
            .filter(names_a_setting)
            .filter(|row| !row.per_game() && !STAYS_GLOBAL.contains(row))
            .collect();
        assert!(
            global.is_empty(),
            "mark these PerGame in rows.rs, or add them to STAYS_GLOBAL if they are about the app or the device: {global:?}"
        );

        let listed: Vec<RowId> = STAYS_GLOBAL
            .iter()
            .copied()
            .filter(|row| {
                row.per_game() || !GAME_PAGES.contains(&row.page()) || !names_a_setting(row)
            })
            .collect();
        assert!(
            listed.is_empty(),
            "STAYS_GLOBAL holds only global settings on game pages: {listed:?}"
        );
    }
}

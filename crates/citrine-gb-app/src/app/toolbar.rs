use super::{Citrine, link, tabs, widgets};
use crate::app::system_files::SystemFile;
use crate::app::tabs::Tab;
use crate::app::ui_state::hotkeys::HotkeyAction;
#[cfg(not(target_arch = "wasm32"))]
use crate::app::ui_state::settings::{
    console_icon, console_name, kind_icon_or_cart, kind_name, other_console,
};
use crate::app::widgets::dev_menu::dev_menu;
use crate::app::widgets::menu_row::menu_row_with;
use crate::app::widgets::shortcut::{shortcut_button, shortcut_hint};
use crate::icons;
#[cfg(not(target_arch = "wasm32"))]
use citrine_gb::rom::header::RomCgbMode;
use egui::WidgetText;
#[cfg(not(target_arch = "wasm32"))]
use egui::containers::menu::SubMenuButton;
use egui::text::{LayoutJob, TextFormat};
use egui::{Align, Button, Color32, FontId, Layout, TextStyle, TextWrapMode, Ui};
use egui_dock::DockState;

const SEPARATOR_SPACING: f32 = 6.0;
const CARET_SCALE: f32 = 0.7;
const RESET_LAYOUT: &str = "Reset layout";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Item {
    Games,
    Open,
    Pause,
    Rewind,
    FastForward,
    Saves,
    Screenshot,
    Record,
    Clip,
    Mute,
    ThisGame,
    Friends,
    Media,
    Settings,
    Focus,
}

const LABELS_GIVE_WAY: [Item; 7] = [
    Item::Focus,
    Item::Media,
    Item::Settings,
    Item::Friends,
    Item::Open,
    Item::Games,
    Item::ThisGame,
];

const FOLDS_INTO_MORE: [Item; 9] = [
    Item::Focus,
    Item::Media,
    Item::Clip,
    Item::Record,
    Item::Screenshot,
    Item::Mute,
    Item::Games,
    Item::Friends,
    Item::Saves,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entry {
    Rom,
    Recent,
    Homebrew,
    File(SystemFile),
    QuickSave,
    NewSlot,
    QuickLoad,
    AllSaves,
    GameInfo,
    GameSettings,
    Cheats,
    Lid,
    Link,
    Reset,
    RomInfo,
    BugReport,
    Achievements,
    Stats,
    SecondScreens,
    Walker,
    Gts,
    BigScreen,
    Dev,
    About,
    ResetLayout,
}

struct Size {
    item: Item,
    icon: f32,
    words: Option<f32>,
}

#[derive(Debug, PartialEq)]
struct Fit {
    shown: Vec<Item>,
    labelled: Vec<Item>,
    folded: Vec<Item>,
}

fn fit(sizes: &[Size], fixed: f32, room: f32) -> Fit {
    let width = |labelled: &[Item], folded: &[Item]| {
        fixed
            + sizes
                .iter()
                .filter(|size| !folded.contains(&size.item))
                .map(|size| match size.words {
                    Some(words) if labelled.contains(&size.item) => words,
                    _ => size.icon,
                })
                .sum::<f32>()
    };
    let mut labelled: Vec<Item> = sizes
        .iter()
        .filter(|size| size.words.is_some())
        .map(|size| size.item)
        .collect();
    let mut folded = Vec::new();
    for item in LABELS_GIVE_WAY {
        if width(&labelled, &folded) <= room {
            break;
        }
        labelled.retain(|other| *other != item);
    }
    for item in FOLDS_INTO_MORE {
        if width(&labelled, &folded) <= room {
            break;
        }
        if sizes.iter().any(|size| size.item == item) {
            folded.push(item);
        }
    }
    let (folded, shown) = sizes
        .iter()
        .map(|size| size.item)
        .partition(|item| folded.contains(item));
    Fit {
        shown,
        labelled,
        folded,
    }
}

fn tidy(entries: Vec<Option<Entry>>) -> Vec<Option<Entry>> {
    let mut out: Vec<Option<Entry>> = Vec::new();
    for entry in entries {
        if entry.is_none() && out.last().is_none_or(Option::is_none) {
            continue;
        }
        out.push(entry);
    }
    if out.last().is_some_and(Option::is_none) {
        out.pop();
    }
    out
}

fn bar_text(ui: &Ui, icon: &str, label: Option<&str>, caret: bool) -> WidgetText {
    let font = TextStyle::Button.resolve(ui.style());
    let plain = TextFormat::simple(font.clone(), Color32::PLACEHOLDER);
    let mut job = LayoutJob::default();
    job.append(icon, 0.0, plain.clone());
    if let Some(label) = label {
        job.append(&format!("  {label}"), 0.0, plain);
    }
    if caret {
        let small = TextFormat {
            font_id: FontId::new(font.size * CARET_SCALE, font.family),
            color: Color32::PLACEHOLDER,
            valign: Align::Center,
            ..Default::default()
        };
        job.append(icons::CARET_DOWN, font.size * 0.3, small);
    }
    WidgetText::from(job)
}

fn button_width(ui: &Ui, text: impl Into<WidgetText>) -> f32 {
    let galley = text.into().into_galley(
        ui,
        Some(TextWrapMode::Extend),
        f32::INFINITY,
        TextStyle::Button,
    );
    galley.size().x + ui.spacing().button_padding.x * 2.0 + ui.spacing().item_spacing.x
}

struct Look {
    icon: &'static str,
    label: String,
    hint: String,
    shortcut: Option<String>,
    enabled: bool,
    dot: bool,
}

impl Look {
    fn row_text(&self) -> String {
        format!("{}  {}", self.icon, self.label)
    }
}

impl Citrine {
    pub(super) fn top_panel(&mut self, ui: &mut Ui) {
        let row = ui.horizontal(|ui| {
            let left = ui.cursor().left();
            let more = self.more_entries();
            let fit = self.bar_fit(ui);
            let (leading, trailing): (Vec<Item>, Vec<Item>) =
                fit.shown.iter().copied().partition(|item| {
                    !matches!(
                        item,
                        Item::Friends | Item::Media | Item::Settings | Item::Focus
                    )
                });

            for item in &leading {
                self.item_button(ui, *item, fit.labelled.contains(item));
                if *item == Item::Open && self.emulator.has_game() {
                    ui.add(egui::Separator::default().spacing(SEPARATOR_SPACING));
                }
            }
            let leading_right = ui.cursor().left();
            let trailing_left = ui
                .with_layout(Layout::right_to_left(Align::Center), |ui| {
                    self.more_button(ui, &fit.folded, &more);
                    for item in trailing.iter().rev() {
                        self.item_button(ui, *item, fit.labelled.contains(item));
                    }
                    ui.min_rect().left()
                })
                .inner;
            left + (leading_right - left) + (ui.max_rect().right() - trailing_left)
        });
        self.ui.toolbar_reach = row.inner.max(row.response.rect.right());
    }

    fn bar_fit(&self, ui: &Ui) -> Fit {
        let sizes: Vec<Size> = self
            .items()
            .iter()
            .map(|item| self.size(ui, *item))
            .collect();
        let mut fixed = button_width(ui, icons::DOTS_THREE);
        if self.emulator.has_game() {
            fixed += SEPARATOR_SPACING * 2.0 + ui.spacing().item_spacing.x;
        }
        fit(&sizes, fixed, ui.available_width())
    }

    fn together(&self) -> bool {
        self.play.session().is_some() || self.emulator.together
    }

    pub(super) fn items(&self) -> Vec<Item> {
        let mut items = vec![Item::Games, Item::Open];
        if self.emulator.has_game() {
            let replay = self.emulator.is_playing_tape();
            if !replay {
                items.push(Item::Pause);
            }
            if !self.together() {
                if !replay && self.emulator.rewind_length.seconds().is_some() {
                    items.push(Item::Rewind);
                }
                items.push(Item::FastForward);
            }
            if !replay {
                items.push(Item::Saves);
            }
            items.extend([
                Item::Screenshot,
                Item::Record,
                Item::Clip,
                Item::Mute,
                Item::ThisGame,
            ]);
        }
        if self.ui.settings.online.is_joined() {
            items.push(Item::Friends);
        }
        items.extend([Item::Media, Item::Settings, Item::Focus]);
        items
    }

    fn size(&self, ui: &Ui, item: Item) -> Size {
        let look = self.item_look(item);
        let caret = Self::menu_of(item);
        Size {
            item,
            icon: button_width(ui, bar_text(ui, look.icon, None, caret)),
            words: self
                .item_label(item)
                .map(|label| button_width(ui, bar_text(ui, look.icon, Some(label), caret))),
        }
    }

    fn menu_of(item: Item) -> bool {
        matches!(item, Item::Open | Item::Saves | Item::ThisGame)
    }

    fn item_label(&self, item: Item) -> Option<&'static str> {
        match item {
            Item::Games => Some(Tab::Games.label()),
            Item::Open => Some("Open"),
            Item::ThisGame => Some("This game"),
            Item::Friends => Some(Tab::Friends.label()),
            Item::Media => Some("Media"),
            Item::Settings => Some("Settings"),
            Item::Focus => Some("Focus"),
            _ => None,
        }
    }

    fn item_look(&self, item: Item) -> Look {
        let hotkeys = &self.ui.settings.hotkeys;
        let action = |action: HotkeyAction, on: bool| Look {
            icon: action.icon(on),
            label: action.hint(on).to_owned(),
            hint: shortcut_hint(hotkeys, action, on),
            shortcut: hotkeys.key_name(action),
            enabled: true,
            dot: false,
        };
        let plain = |icon, label: &str| Look {
            icon,
            label: label.to_owned(),
            hint: label.to_owned(),
            shortcut: None,
            enabled: true,
            dot: false,
        };
        match item {
            Item::Games => plain(icons::CARTRIDGE, Tab::Games.label()),
            Item::Open => plain(icons::FOLDER, "Open"),
            Item::Pause => action(HotkeyAction::TogglePause, self.emulator.is_running()),
            Item::Rewind => action(HotkeyAction::Rewind, false),
            Item::FastForward => action(HotkeyAction::FastForward, false),
            Item::Saves => Look {
                hint: match hotkeys.quick_keys() {
                    Some(keys) => format!("Saves ({keys} quick save / load)"),
                    None => "Saves".to_owned(),
                },
                ..plain(icons::FLOPPY_DISK, "Saves")
            },
            Item::Screenshot => action(HotkeyAction::Screenshot, false),
            Item::Record => {
                let recording = self.emulator.is_recording();
                Look {
                    icon: if recording {
                        icons::STOP
                    } else {
                        icons::RECORD
                    },
                    label: if recording {
                        "Stop recording"
                    } else {
                        "Record"
                    }
                    .to_owned(),
                    ..action(HotkeyAction::ToggleRecording, false)
                }
            }
            Item::Clip => {
                let blocked = self.clip_blocked();
                Look {
                    enabled: blocked.is_none(),
                    hint: blocked
                        .unwrap_or_else(|| shortcut_hint(hotkeys, HotkeyAction::SaveClip, false)),
                    ..action(HotkeyAction::SaveClip, false)
                }
            }
            Item::Mute => action(HotkeyAction::ToggleMute, self.ui.settings.muted),
            Item::ThisGame => plain(icons::THIS_GAME, "This game"),
            Item::Friends => {
                let invited = self.play.has_unseen_invite();
                Look {
                    hint: tabs::chat::attention_hint(&self.online, invited).to_string(),
                    dot: self.online.needs_attention() || invited,
                    ..plain(icons::USERS, Tab::Friends.label())
                }
            }
            Item::Media => Look {
                hint: "Screenshots and recordings".to_owned(),
                ..plain(icons::IMAGES, "Media")
            },
            Item::Settings => Look {
                dot: self.ui.update.phase().waiting(),
                ..plain(icons::GEAR, "Settings")
            },
            Item::Focus => Look {
                label: HotkeyAction::ToggleFocusMode.label().to_owned(),
                ..action(HotkeyAction::ToggleFocusMode, false)
            },
        }
    }

    fn games_shown(&self) -> bool {
        tabs::games::idle(&self.emulator, &self.play)
            || (!self.ui.games.hidden && self.dock.find_tab(&Tab::Games).is_some())
    }

    fn item_button(&mut self, ui: &mut Ui, item: Item, labelled: bool) {
        let look = self.item_look(item);
        let label = if labelled {
            self.item_label(item)
        } else {
            None
        };
        let text = bar_text(ui, look.icon, label, Self::menu_of(item));
        let hotkeys = &self.ui.settings.hotkeys;
        let has_game = self.emulator.has_game();
        let response = match item {
            Item::Open => {
                ui.menu_button(text, |ui| self.entries_menu(ui, Item::Open))
                    .response
            }
            Item::Saves => {
                ui.menu_button(text, |ui| self.entries_menu(ui, Item::Saves))
                    .response
            }
            Item::ThisGame => {
                ui.menu_button(text, |ui| self.entries_menu(ui, Item::ThisGame))
                    .response
            }
            Item::Pause => {
                let running = self.emulator.is_running();
                shortcut_button(ui, hotkeys, HotkeyAction::TogglePause, running, has_game)
            }
            Item::Rewind => {
                self.rewind_pressed |= self.rewind_button(ui);
                return;
            }
            Item::FastForward => {
                self.fast_forward_button(ui);
                return;
            }
            Item::Screenshot => {
                shortcut_button(ui, hotkeys, HotkeyAction::Screenshot, false, has_game)
            }
            Item::Record => self.recording_button(ui, None),
            Item::Clip => {
                let look = self.item_look(Item::Clip);
                let button = ui.add_enabled(look.enabled, Button::new(look.icon));
                if look.enabled {
                    button.on_hover_text(look.hint)
                } else {
                    button.on_disabled_hover_text(look.hint)
                }
            }
            Item::Mute => {
                let muted = self.ui.settings.muted;
                shortcut_button(ui, hotkeys, HotkeyAction::ToggleMute, muted, true)
            }
            Item::Games => ui.add(Button::new(text).selected(self.games_shown())),
            Item::Friends | Item::Media | Item::Settings => {
                let lit = Self::pane_of(item).is_some_and(|tab| self.dock.find_tab(&tab).is_some());
                ui.add(Button::new(text).selected(lit))
            }
            Item::Focus => ui.add(Button::new(text)),
        };
        let response = match item {
            Item::Pause | Item::Screenshot | Item::Record | Item::Clip | Item::Mute => response,
            _ => response.on_hover_text(&look.hint),
        };
        if look.dot {
            widgets::avatar::status_dot(ui, response.rect);
        }
        if !Self::menu_of(item) && response.clicked() {
            self.run_item(item);
        }
    }

    fn item_row(&mut self, ui: &mut Ui, item: Item) {
        let look = self.item_look(item);
        if Self::menu_of(item) {
            let label = self.item_label(item).unwrap_or(look.label.as_str());
            let text = format!("{}  {label}", look.icon);
            ui.menu_button(text, |ui| self.entries_menu(ui, item));
            return;
        }
        let response = ui
            .add_enabled_ui(look.enabled, |ui| {
                menu_row_with(ui, look.row_text(), look.shortcut.as_deref(), None)
            })
            .inner
            .on_hover_text(&look.hint);
        if look.dot {
            widgets::avatar::status_dot(ui, response.rect);
        }
        if response.clicked() {
            self.run_item(item);
        }
    }

    fn run_item(&mut self, item: Item) {
        match item {
            Item::Games => self.toggle_games(),
            Item::Pause => self.run_hotkey(HotkeyAction::TogglePause),
            Item::Screenshot => self.run_hotkey(HotkeyAction::Screenshot),
            Item::Record => self.run_hotkey(HotkeyAction::ToggleRecording),
            Item::Clip => self.run_hotkey(HotkeyAction::SaveClip),
            Item::Mute => self.run_hotkey(HotkeyAction::ToggleMute),
            Item::Friends | Item::Media | Item::Settings => {
                if let Some(tab) = Self::pane_of(item) {
                    self.toggle_pane(tab);
                }
            }
            Item::Focus => self.toggle_focus_mode(),
            Item::Open | Item::Saves | Item::ThisGame | Item::Rewind | Item::FastForward => {}
        }
    }

    fn pane_of(item: Item) -> Option<Tab> {
        match item {
            Item::Friends => Some(Tab::Friends),
            Item::Media => Some(Tab::Media),
            Item::Settings => Some(Tab::Settings),
            _ => None,
        }
    }

    fn bring_forward(&mut self, tab: Tab) -> bool {
        let Some((surface, node, index)) = self.dock.find_tab(&tab) else {
            return false;
        };
        let covered = matches!(
            &self.dock[surface][node],
            egui_dock::Node::Leaf(leaf) if leaf.active != index
        );
        covered && self.focus_tab(tab)
    }

    fn toggle_pane(&mut self, tab: Tab) {
        if self.bring_forward(tab) {
            return;
        }
        match self.dock.find_tab(&tab) {
            Some(location) => {
                self.dock.remove_tab(location);
            }
            None => self.open_tab(tab),
        }
    }

    fn toggle_games(&mut self) {
        if tabs::games::idle(&self.emulator, &self.play) {
            self.ui.games.hidden = false;
            return;
        }
        if self.bring_forward(Tab::Games) {
            return;
        }
        if self.dock.find_tab(&Tab::Games).is_some() {
            self.ui.games.hidden = true;
        } else {
            self.show_games();
        }
    }

    fn menu_entries(&self, item: Item) -> Vec<Option<Entry>> {
        let mut entries = Vec::new();
        match item {
            Item::Open => {
                entries.push(Some(Entry::Rom));
                if cfg!(not(target_arch = "wasm32")) {
                    entries.push(Some(Entry::Recent));
                }
                entries.extend([Some(Entry::Homebrew), None]);
                entries.extend(
                    SystemFile::all()
                        .into_iter()
                        .map(|file| Some(Entry::File(file))),
                );
            }
            Item::Saves => entries.extend([
                Some(Entry::QuickSave),
                Some(Entry::NewSlot),
                Some(Entry::QuickLoad),
                None,
                Some(Entry::AllSaves),
            ]),
            Item::ThisGame => {
                if self.game_info.recognised() {
                    entries.push(Some(Entry::GameInfo));
                }
                entries.push(Some(Entry::GameSettings));
                entries.push(Some(Entry::Cheats));
                if self.emulator.lid().is_some() {
                    entries.push(Some(Entry::Lid));
                }
                entries.extend([
                    None,
                    Some(Entry::Link),
                    Some(Entry::Reset),
                    None,
                    Some(Entry::RomInfo),
                    Some(Entry::BugReport),
                ]);
            }
            _ => {}
        }
        tidy(entries)
    }

    fn more_entries(&self) -> Vec<Option<Entry>> {
        let mut entries = Vec::new();
        if cfg!(not(target_arch = "wasm32")) {
            entries.push(Some(Entry::Achievements));
        }
        entries.extend([Some(Entry::Stats), Some(Entry::SecondScreens)]);
        if cfg!(feature = "nds") {
            entries.push(Some(Entry::Walker));
            if self.ui.settings.online.is_joined() && self.ui.settings.ds_online.gts_here() {
                entries.push(Some(Entry::Gts));
            }
        }
        entries.push(Some(Entry::BigScreen));
        if self.ui.settings.dev_mode {
            entries.push(Some(Entry::Dev));
        }
        entries.extend([None, Some(Entry::About), Some(Entry::ResetLayout)]);
        tidy(entries)
    }

    fn entry_look(&self, entry: Entry) -> Look {
        let hotkeys = &self.ui.settings.hotkeys;
        let has_game = self.emulator.has_game();
        let plain = |icon, label: &str| Look {
            icon,
            label: label.to_owned(),
            hint: label.to_owned(),
            shortcut: None,
            enabled: true,
            dot: false,
        };
        let action = |icon, action: HotkeyAction, on: bool| Look {
            shortcut: hotkeys.key_name(action),
            enabled: has_game,
            ..plain(icon, action.hint(on))
        };
        match entry {
            Entry::Rom => plain(icons::FOLDER_OPEN, "ROM"),
            Entry::Recent => plain(icons::CLOCK_COUNTER_CLOCKWISE, "Recent"),
            Entry::Homebrew => plain(icons::HOMEBREW, Tab::Homebrew.label()),
            Entry::File(file) => plain(file.icon(), file.noun()),
            Entry::QuickSave => action(icons::TRAY_ARROW_DOWN, HotkeyAction::QuickSave, false),
            Entry::NewSlot => action(icons::PLUS_CIRCLE, HotkeyAction::QuickSaveNewSlot, false),
            Entry::QuickLoad => action(icons::TRAY_ARROW_UP, HotkeyAction::QuickLoad, false),
            Entry::AllSaves => plain(icons::FLOPPY_DISK, "All saves"),
            Entry::GameInfo => plain(icons::INFO, Tab::GameInfo.label()),
            Entry::GameSettings => plain(icons::GEAR_FINE, Tab::ThisGame.label()),
            Entry::Cheats => plain(icons::MAGIC_WAND, Tab::Cheats.label()),
            Entry::Lid => {
                let closed = self.emulator.lid().unwrap_or(false);
                Look {
                    enabled: true,
                    ..action(
                        HotkeyAction::ToggleLid.icon(closed),
                        HotkeyAction::ToggleLid,
                        closed,
                    )
                }
            }
            Entry::Link => {
                let entry = self.link_entry();
                let label = match entry {
                    link::LinkEntry::Blocked(_) | link::LinkEntry::Ready => "Link a second game",
                    link::LinkEntry::Open if self.emulator.partnered() => "Event partner",
                    link::LinkEntry::Open => "Linked game",
                };
                Look {
                    hint: match entry {
                        link::LinkEntry::Blocked(why) => why.to_owned(),
                        _ => label.to_owned(),
                    },
                    enabled: !matches!(entry, link::LinkEntry::Blocked(_)),
                    ..plain(tabs::linked::LINK_ICON, label)
                }
            }
            Entry::Reset => action(
                HotkeyAction::SoftReset.icon(false),
                HotkeyAction::SoftReset,
                false,
            ),
            Entry::RomInfo => plain(icons::FILE_TEXT, Tab::RomInfo.label()),
            Entry::BugReport => Look {
                hint: super::report::GAME_REPORT_NOTE.to_owned(),
                enabled: has_game,
                ..plain(icons::BUG, super::report::SAVE_GAME_REPORT)
            },
            Entry::Achievements => plain(icons::TROPHY, Tab::Achievements.label()),
            Entry::Stats => plain(icons::CHART_BAR, "Stats"),
            Entry::SecondScreens => plain(icons::DEVICES, Tab::SecondScreens.title()),
            Entry::Walker => plain(icons::POKEWALKER, Tab::Walker.title()),
            Entry::Gts => Look {
                dot: self.gts_traded(),
                ..plain(icons::GLOBE_HEMISPHERE_EAST, Tab::Gts.label())
            },
            Entry::BigScreen => plain(icons::TELEVISION_SIMPLE, "Big screen"),
            Entry::Dev => plain(icons::CODE, "Dev tools"),
            Entry::About => plain(icons::INFO, "About"),
            Entry::ResetLayout => plain(icons::LAYOUT, RESET_LAYOUT),
        }
    }

    fn gts_traded(&self) -> bool {
        self.online.gts_mine().is_some_and(|mine| {
            mine.iter()
                .any(|d| matches!(d.state, citrine_gb_net::GtsState::Traded { .. }))
        })
    }

    fn entries_menu(&mut self, ui: &mut Ui, item: Item) {
        ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
        for entry in self.menu_entries(item) {
            self.entry_row(ui, entry);
        }
    }

    fn more_button(&mut self, ui: &mut Ui, folded: &[Item], more: &[Option<Entry>]) {
        let dot = folded.iter().any(|item| self.item_look(*item).dot)
            || more
                .iter()
                .flatten()
                .any(|entry| self.entry_look(*entry).dot);
        let response = ui
            .menu_button(icons::DOTS_THREE, |ui| {
                ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
                for item in folded {
                    self.item_row(ui, *item);
                }
                if !folded.is_empty() {
                    ui.separator();
                }
                for entry in more {
                    self.entry_row(ui, *entry);
                }
            })
            .response
            .on_hover_text("More");
        if dot {
            widgets::avatar::status_dot(ui, response.rect);
        }
    }

    fn entry_row(&mut self, ui: &mut Ui, entry: Option<Entry>) {
        let Some(entry) = entry else {
            ui.separator();
            return;
        };
        let look = self.entry_look(entry);
        match entry {
            Entry::Recent => {
                #[cfg(not(target_arch = "wasm32"))]
                ui.menu_button(look.row_text(), |ui| self.recent_menu(ui));
                return;
            }
            Entry::Dev => {
                ui.menu_button(look.row_text(), |ui| {
                    if let Some(tab) = dev_menu(ui) {
                        self.open_tab(tab);
                    }
                });
                return;
            }
            _ => {}
        }
        let response = ui
            .add_enabled_ui(look.enabled, |ui| {
                menu_row_with(ui, look.row_text(), look.shortcut.as_deref(), None)
            })
            .inner
            .on_hover_text(&look.hint)
            .on_disabled_hover_text(&look.hint);
        if look.dot {
            widgets::avatar::status_dot(ui, response.rect);
        }
        if response.clicked() {
            self.run_entry(entry);
        }
    }

    fn run_entry(&mut self, entry: Entry) {
        match entry {
            Entry::Rom => self.pick_rom(),
            Entry::File(file) => self.pick_system_file(file),
            Entry::Homebrew => self.open_tab(Tab::Homebrew),
            Entry::QuickSave => self.run_hotkey(HotkeyAction::QuickSave),
            Entry::NewSlot => self.run_hotkey(HotkeyAction::QuickSaveNewSlot),
            Entry::QuickLoad => self.run_hotkey(HotkeyAction::QuickLoad),
            Entry::AllSaves => self.open_tab(Tab::Saves),
            Entry::GameInfo => {
                self.open_tab(Tab::GameInfo);
                self.widen_for_game_info();
            }
            Entry::GameSettings => self.open_tab(Tab::ThisGame),
            Entry::Cheats => self.open_tab(Tab::Cheats),
            Entry::Lid => self.run_hotkey(HotkeyAction::ToggleLid),
            Entry::Link => self.pick_second_game(false),
            Entry::Reset => self.run_hotkey(HotkeyAction::SoftReset),
            Entry::RomInfo => self.open_tab(Tab::RomInfo),
            Entry::BugReport => self.save_game_report(),
            Entry::Achievements => self.open_tab(Tab::Achievements),
            Entry::Stats => self.open_tab(Tab::Stats),
            Entry::SecondScreens => self.open_tab(Tab::SecondScreens),
            Entry::Walker => {
                #[cfg(feature = "nds")]
                self.open_walker();
            }
            Entry::Gts => self.open_tab(Tab::Gts),
            Entry::BigScreen => self.set_big_screen(true),
            Entry::About => self.open_tab(Tab::Info),
            Entry::ResetLayout => self.reset_layout(),
            Entry::Recent | Entry::Dev => {}
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn recent_menu(&mut self, ui: &mut Ui) {
        if let Some((path, console)) = self.recent_pick(ui) {
            if let Some((key, model)) = console {
                self.ui.settings.set_game_console(&key, model);
            }
            self.load_recent(path);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn recent_pick(&mut self, ui: &mut Ui) -> Option<RecentPick> {
        ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
        let library = &self.ui.library;
        self.ui
            .recent
            .prune_except(|path| library.is_archive_row(path));
        self.ui.recent.name_from_files();
        if self.ui.recent.is_empty() {
            ui.add_enabled(false, Button::new("No recent ROMs"));
            return None;
        }
        let mut chosen = None;
        for entry in self.ui.recent.entries() {
            let name = format!(
                "{}  {}",
                kind_icon_or_cart(entry.console),
                entry.display_name()
            );
            let clicked = match entry.rom_key.as_deref() {
                Some(key) => {
                    let model = self.ui.settings.console_for(key, RomCgbMode::CgbAndGb);
                    let button = Button::new(name).right_text(SubMenuButton::RIGHT_ARROW);
                    SubMenuButton::from_button(button)
                        .ui(ui, |ui| {
                            for model in [model, other_console(model)] {
                                if ui
                                    .button(format!(
                                        "{}  Play on {}",
                                        console_icon(model),
                                        console_name(model)
                                    ))
                                    .clicked()
                                {
                                    chosen =
                                        Some((entry.path.clone(), Some((key.to_owned(), model))));
                                }
                            }
                        })
                        .0
                        .clicked()
                }
                None => ui
                    .add(Button::new(name))
                    .on_hover_text(recent_hint(entry))
                    .clicked(),
            };
            if clicked {
                chosen = Some((entry.path.clone(), None));
            }
        }
        chosen
    }

    fn reset_layout(&mut self) {
        self.dock = DockState::new(vec![Tab::GameBoy]);
        if self.ui.settings.online.is_joined() {
            self.open_tab(Tab::Friends);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
type RecentPick = (
    std::path::PathBuf,
    Option<(String, citrine_gb::gb::GbModel)>,
);

#[cfg(not(target_arch = "wasm32"))]
fn recent_hint(entry: &crate::app::ui_state::recent::RecentRom) -> String {
    let path = entry.path.display();
    match entry.console {
        Some(kind) => format!("{}\n{path}", kind_name(kind)),
        None => path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::boot_rom::BootRom;
    use crate::app::tests::{load_game, phone_app};
    use crate::app::ui_state::settings::LayoutMode;
    use egui::{Context, RawInput, Rect, pos2, vec2};

    const WEB_ZOOM_AT_1280: f32 = 2.2;

    fn sized(items: &[(Item, Option<f32>)]) -> Vec<Size> {
        items
            .iter()
            .map(|(item, words)| Size {
                item: *item,
                icon: 30.0,
                words: *words,
            })
            .collect()
    }

    fn game_row() -> Vec<Size> {
        sized(&[
            (Item::Games, Some(80.0)),
            (Item::Open, Some(80.0)),
            (Item::Pause, None),
            (Item::Rewind, None),
            (Item::FastForward, None),
            (Item::Saves, None),
            (Item::Screenshot, None),
            (Item::Record, None),
            (Item::Clip, None),
            (Item::Mute, None),
            (Item::ThisGame, Some(100.0)),
            (Item::Friends, Some(80.0)),
            (Item::Media, Some(80.0)),
            (Item::Settings, Some(80.0)),
            (Item::Focus, Some(80.0)),
        ])
    }

    fn desktop(name: &str) -> Citrine {
        let mut app = phone_app(name);
        app.ui.settings.layout = LayoutMode::Desktop;
        app
    }

    fn with_game(name: &str) -> Citrine {
        let mut app = desktop(name);
        load_game(
            &mut app,
            &citrine_gb_gamedata::fixtures::crystal_cartridge(),
            None,
        );
        app
    }

    fn sign_in(app: &mut Citrine) {
        let account = citrine_gb_net::Account {
            online_id: citrine_gb_net::OnlineId::new(0x1234_5678).unwrap(),
            display_name: "Clara".into(),
            storage_used: 0,
            quota_bytes: 0,
            two_factor: false,
            codes_left: 0,
        };
        app.ui.settings.online.remember(
            &crate::online::address::ServerAddress::parse("relay.example").unwrap(),
            &citrine_gb_net::RelayId::random(),
            "pw",
            &account,
            &Default::default(),
        );
    }

    fn online(name: &str) -> Citrine {
        let mut app = with_game(name);
        sign_in(&mut app);
        app.emulator.together = true;
        app
    }

    fn contexts() -> Vec<(&'static str, Citrine)> {
        [
            ("no game", desktop("toolbar-none")),
            ("game", with_game("toolbar-game")),
            ("online", online("toolbar-online")),
        ]
        .into_iter()
        .chain(ds_context())
        .collect()
    }

    #[cfg(feature = "nds")]
    fn ds_context() -> Option<(&'static str, Citrine)> {
        let mut ds = desktop("toolbar-ds");
        ds.ui.settings.dev_mode = true;
        ds.emulator
            .load_nds(include_bytes!(
                "../../fixtures/nds/blocksds-console-both-screens.nds"
            ))
            .unwrap();
        Some(("ds", ds))
    }

    #[cfg(not(feature = "nds"))]
    fn ds_context() -> Option<(&'static str, Citrine)> {
        None
    }

    fn fit_at(app: &Citrine, width: f32) -> Fit {
        let ctx = Context::default();
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(width, 600.0))),
            ..Default::default()
        };
        let mut fit = None;
        let _ = ctx.run(input, |ctx| {
            egui::TopBottomPanel::top("fit").show(ctx, |ui| {
                ui.horizontal(|ui| fit = Some(app.bar_fit(ui)));
            });
        });
        fit.unwrap()
    }

    fn entries_reached(app: &Citrine, fit: &Fit) -> Vec<Entry> {
        let mut entries: Vec<Entry> = app.more_entries().into_iter().flatten().collect();
        for item in fit.shown.iter().chain(&fit.folded) {
            entries.extend(app.menu_entries(*item).into_iter().flatten());
        }
        entries
    }

    #[test]
    fn a_wide_bar_keeps_every_label_and_folds_nothing() {
        let fit = fit(&game_row(), 30.0, 2000.0);
        assert_eq!(fit.shown.len(), 15);
        assert!(fit.folded.is_empty());
        assert_eq!(fit.labelled.len(), 7);
    }

    #[test]
    fn labels_drop_from_focus_first_to_this_game_last() {
        let row = game_row();
        let all: f32 = 8.0 * 30.0 + 6.0 * 80.0 + 100.0 + 30.0;
        let fit = fit(&row, 30.0, all - 40.0);
        assert!(!fit.labelled.contains(&Item::Focus));
        assert!(fit.labelled.contains(&Item::Media));
        let fit = super::fit(&row, 30.0, 14.0 * 30.0 + 100.0 + 30.0 + 10.0);
        assert_eq!(fit.labelled, vec![Item::ThisGame]);
        assert!(fit.folded.is_empty());
    }

    #[test]
    fn a_narrow_bar_folds_into_more_but_keeps_settings_pause_open_and_this_game() {
        let fit = fit(&game_row(), 30.0, 8.0 * 30.0);
        assert!(fit.labelled.is_empty());
        for kept in [Item::Settings, Item::Pause, Item::Open, Item::ThisGame] {
            assert!(fit.shown.contains(&kept), "{kept:?} folded");
        }
        assert!(fit.folded.contains(&Item::Focus) && fit.folded.contains(&Item::Media));
        let order: Vec<Item> = game_row()
            .iter()
            .map(|size| size.item)
            .filter(|item| fit.folded.contains(item))
            .collect();
        assert_eq!(fit.folded, order, "More keeps the bar's order");
    }

    #[test]
    fn with_no_game_the_bar_has_no_play_controls_and_no_this_game() {
        let app = desktop("toolbar-no-game");
        assert_eq!(
            app.items(),
            vec![
                Item::Games,
                Item::Open,
                Item::Media,
                Item::Settings,
                Item::Focus
            ]
        );
    }

    #[test]
    fn a_game_brings_the_play_controls_and_this_game() {
        let app = with_game("toolbar-play-group");
        let items = app.items();
        for item in [
            Item::Pause,
            Item::FastForward,
            Item::Saves,
            Item::Screenshot,
            Item::Record,
            Item::Clip,
            Item::Mute,
            Item::ThisGame,
        ] {
            assert!(items.contains(&item), "{item:?} missing");
        }
        assert_eq!(
            items.contains(&Item::Rewind),
            app.emulator.rewind_length.seconds().is_some()
        );
    }

    #[test]
    fn clip_length_off_greys_out_save_a_clip_and_says_where_to_turn_it_on() {
        let mut app = with_game("toolbar-clip-off");
        assert!(app.item_look(Item::Clip).enabled);
        app.emulator.rewind_length = crate::emulator::rewind::RewindLength::Off;
        assert!(
            app.item_look(Item::Clip).enabled,
            "clips work without rewind"
        );
        app.emulator.clip_length = crate::emulator::rewind::ClipLength::Off;
        let look = app.item_look(Item::Clip);
        assert!(!look.enabled);
        assert_eq!(look.hint, crate::app::clip::CLIPS_OFF);
    }

    #[test]
    fn playing_together_hides_rewind_and_fast_forward() {
        let mut app = with_game("toolbar-together");
        assert!(app.items().contains(&Item::FastForward));
        app.emulator.together = true;
        let items = app.items();
        assert!(!items.contains(&Item::Rewind));
        assert!(!items.contains(&Item::FastForward));
        assert!(items.contains(&Item::Pause) && items.contains(&Item::Saves));
    }

    #[test]
    fn friends_shows_only_when_signed_in() {
        let mut app = desktop("toolbar-friends");
        assert!(!app.items().contains(&Item::Friends));
        sign_in(&mut app);
        assert!(app.items().contains(&Item::Friends));
    }

    #[test]
    fn every_command_stays_reachable_at_every_width() {
        let always = [
            Entry::Rom,
            Entry::Homebrew,
            Entry::File(SystemFile::BootRom(BootRom::Gb)),
            Entry::Stats,
            Entry::SecondScreens,
            Entry::BigScreen,
            Entry::About,
            Entry::ResetLayout,
        ];
        let with_game = [
            Entry::QuickSave,
            Entry::NewSlot,
            Entry::QuickLoad,
            Entry::AllSaves,
            Entry::GameSettings,
            Entry::Cheats,
            Entry::Link,
            Entry::Reset,
            Entry::RomInfo,
            Entry::BugReport,
        ];
        for (name, app) in contexts() {
            for width in [1280.0, 560.0, 300.0] {
                let fit = fit_at(&app, width);
                let mut items: Vec<Item> = fit.shown.iter().chain(&fit.folded).copied().collect();
                items.sort_by_key(|item| *item as u8);
                let mut wanted = app.items();
                wanted.sort_by_key(|item| *item as u8);
                assert_eq!(items, wanted, "{name} at {width}");
                assert!(fit.shown.contains(&Item::Settings), "{name} at {width}");
                assert!(fit.shown.contains(&Item::Open), "{name} at {width}");
                let entries = entries_reached(&app, &fit);
                let mut needed = always.to_vec();
                if app.emulator.has_game() {
                    needed.extend(with_game);
                    assert!(fit.shown.contains(&Item::Pause), "{name} at {width}");
                    assert!(fit.shown.contains(&Item::ThisGame), "{name} at {width}");
                }
                if app.ui.settings.dev_mode {
                    needed.push(Entry::Dev);
                }
                if cfg!(feature = "nds") && app.emulator.lid().is_some() {
                    needed.push(Entry::Lid);
                }
                for entry in needed {
                    assert!(entries.contains(&entry), "{name} at {width}: {entry:?}");
                }
            }
        }
    }

    #[test]
    fn a_1280_bar_folds_nothing_and_keeps_its_labels() {
        for (name, app) in contexts() {
            let fit = fit_at(&app, 1280.0);
            assert!(fit.folded.is_empty(), "{name}: {:?}", fit.folded);
            assert!(fit.labelled.contains(&Item::Settings), "{name}");
        }
    }

    #[test]
    fn every_shortcut_shows_beside_its_command() {
        let app = with_game("toolbar-shortcuts");
        let hotkeys = &app.ui.settings.hotkeys;
        for (item, action) in [
            (Item::Pause, HotkeyAction::TogglePause),
            (Item::Rewind, HotkeyAction::Rewind),
            (Item::FastForward, HotkeyAction::FastForward),
            (Item::Screenshot, HotkeyAction::Screenshot),
            (Item::Record, HotkeyAction::ToggleRecording),
            (Item::Clip, HotkeyAction::SaveClip),
            (Item::Mute, HotkeyAction::ToggleMute),
            (Item::Focus, HotkeyAction::ToggleFocusMode),
        ] {
            let key = hotkeys.key_name(action).expect("a default key");
            assert_eq!(app.item_look(item).shortcut, Some(key), "{item:?}");
        }
        for (entry, key) in [
            (Entry::QuickSave, "F8"),
            (Entry::NewSlot, "Shift+F8"),
            (Entry::QuickLoad, "F9"),
            (Entry::Reset, "Ctrl+R"),
        ] {
            assert_eq!(
                app.entry_look(entry).shortcut.as_deref(),
                Some(key),
                "{entry:?}"
            );
        }
    }

    #[test]
    fn this_game_menu_rows_run_their_command() {
        let mut app = with_game("toolbar-this-game");
        app.run_entry(Entry::GameSettings);
        assert!(app.dock.find_tab(&Tab::ThisGame).is_some());
        app.run_entry(Entry::Cheats);
        assert!(app.dock.find_tab(&Tab::Cheats).is_some());
        app.run_entry(Entry::RomInfo);
        assert!(app.dock.find_tab(&Tab::RomInfo).is_some());
        app.run_entry(Entry::AllSaves);
        assert!(app.dock.find_tab(&Tab::Saves).is_some());
        let muted = app.ui.settings.muted;
        app.run_item(Item::Mute);
        assert_ne!(app.ui.settings.muted, muted);
    }

    fn web_frame(app: &mut Citrine, ctx: &Context, width: f32, height: f32) {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(width, height))),
            max_texture_side: Some(8192),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            let touch = app.is_mobile(ctx);
            crate::app::mobile::touch_spacing(ctx, touch);
            app.render(ctx);
        });
    }

    #[test]
    fn the_toolbar_never_runs_past_the_window() {
        for (name, mut app) in contexts() {
            for (css, zoom) in [
                (1280.0, 1.0),
                (560.0, 1.0),
                (1280.0, WEB_ZOOM_AT_1280),
                (600.0, WEB_ZOOM_AT_1280),
            ] {
                let width = css / zoom;
                let ctx = Context::default();
                ctx.set_zoom_factor(zoom);
                app.ui.toolbar_reach = 0.0;
                for _ in 0..3 {
                    web_frame(&mut app, &ctx, width, 800.0 / zoom);
                }
                assert!(!app.is_mobile(&ctx), "{name} at {css}: phone layout");
                assert!(
                    app.ui.toolbar_reach > 0.0 && app.ui.toolbar_reach <= width + 0.5,
                    "{name} at {css} x{zoom}: the toolbar reaches {} of {width}",
                    app.ui.toolbar_reach
                );
            }
        }
    }

    #[test]
    fn the_games_button_turns_the_pane_on_and_off_with_a_game() {
        let mut app = desktop("toolbar-games");
        assert!(app.items().contains(&Item::Games));
        assert!(
            app.games_shown(),
            "with no game the games list fills the window"
        );
        app.toggle_games();
        assert!(!app.ui.games.hidden, "it never hides the only screen");
        load_game(
            &mut app,
            &citrine_gb_gamedata::fixtures::crystal_cartridge(),
            None,
        );
        app.sync_games_tab();
        app.toggle_games();
        app.sync_games_tab();
        assert!(app.dock.find_tab(&Tab::Games).is_none());
        assert!(!app.games_shown());
        app.toggle_games();
        app.sync_games_tab();
        assert!(app.dock.find_tab(&Tab::Games).is_some());
        assert!(app.games_shown());
    }

    #[test]
    fn the_games_button_brings_a_covered_games_pane_forward() {
        let mut app = with_game("toolbar-games-forward");
        app.sync_games_tab();
        app.open_tab(Tab::Cheats);
        let (surface, node, games) = app.dock.find_tab(&Tab::Games).unwrap();
        assert_eq!(app.dock.find_tab(&Tab::Cheats).unwrap().1, node);
        app.toggle_games();
        app.sync_games_tab();
        assert!(!app.ui.games.hidden);
        let egui_dock::Node::Leaf(leaf) = &app.dock[surface][node] else {
            panic!("the games pane is not in a leaf");
        };
        assert_eq!(leaf.active, games);
    }

    #[test]
    fn the_games_button_shows_a_closed_games_pane_in_front() {
        let mut app = with_game("toolbar-games-shown");
        app.sync_games_tab();
        app.open_tab(Tab::Cheats);
        app.ui.games.hidden = true;
        app.sync_games_tab();
        let game = app.dock.find_tab(&Tab::GameBoy).unwrap();
        app.dock.set_focused_node_and_surface((game.0, game.1));
        app.toggle_games();
        app.sync_games_tab();
        crate::app::tests::assert_in_front(&app, Tab::Games);
    }

    #[test]
    fn friends_media_and_settings_light_up_and_toggle_like_games() {
        let mut app = with_game("toolbar-panes");
        sign_in(&mut app);
        for item in [Item::Friends, Item::Media, Item::Settings] {
            let tab = Citrine::pane_of(item).unwrap();
            if let Some(location) = app.dock.find_tab(&tab) {
                app.dock.remove_tab(location);
            }
            app.run_item(item);
            let (surface, node, index) = app.dock.find_tab(&tab).expect("a click opens it");
            if let egui_dock::Node::Leaf(leaf) = &mut app.dock[surface][node] {
                leaf.tabs.push(Tab::Cheats);
                leaf.active = egui_dock::TabIndex(leaf.tabs.len() - 1);
            }
            app.run_item(item);
            let egui_dock::Node::Leaf(leaf) = &app.dock[surface][node] else {
                panic!("{tab:?} is not in a leaf");
            };
            assert_eq!(
                leaf.active, index,
                "a click brings a covered {tab:?} forward"
            );
            app.run_item(item);
            assert!(
                app.dock.find_tab(&tab).is_none(),
                "a click on a lit {tab:?} closes it"
            );
            if let Some(location) = app.dock.find_tab(&Tab::Cheats) {
                app.dock.remove_tab(location);
            }
        }
    }

    #[cfg(feature = "nds")]
    #[test]
    fn a_ds_game_puts_the_lid_in_this_game() {
        let (_, ds) = contexts().pop().unwrap();
        assert!(ds.menu_entries(Item::ThisGame).contains(&Some(Entry::Lid)));
        let gb = with_game("toolbar-gb-lid");
        assert!(!gb.menu_entries(Item::ThisGame).contains(&Some(Entry::Lid)));
    }

    #[test]
    fn open_lists_every_file_the_build_can_open_by_name() {
        let app = desktop("toolbar-open-files");
        let labels: Vec<String> = app
            .menu_entries(Item::Open)
            .into_iter()
            .flatten()
            .filter(|entry| matches!(entry, Entry::File(_)))
            .map(|entry| app.entry_look(entry).label)
            .collect();
        let mut expected = vec!["GB boot ROM", "GBC boot ROM"];
        if cfg!(feature = "gba") {
            expected.push("GBA BIOS");
        }
        if cfg!(feature = "nds") {
            expected.extend(["DS ARM9 BIOS", "DS ARM7 BIOS", "DS firmware"]);
        }
        assert_eq!(labels, expected);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn recent_frame(
        ctx: &Context,
        app: &mut Citrine,
        events: Vec<egui::Event>,
    ) -> (Option<RecentPick>, Vec<(String, Rect)>) {
        let mut picked = None;
        let output = ctx.run(
            RawInput {
                events,
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| picked = app.recent_pick(ui));
            },
        );
        let texts = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => Some((
                    text.galley.text().to_owned(),
                    text.galley.rect.translate(text.pos.to_vec2()),
                )),
                _ => None,
            })
            .collect();
        (picked, texts)
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_click_on_a_recent_row_plays_on_the_current_model() {
        use citrine_gb::console::ConsoleKind;
        use citrine_gb::gb::GbModel;
        let mut app = desktop("recent-row-click");
        let dir = crate::storage::scratch_dir().join("citrine-recent-row-click");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tetris-dx.gbc");
        std::fs::write(&path, [0u8; 64]).unwrap();
        app.ui.settings.set_game_console("KEY-DX", GbModel::Cgb);
        app.ui.recent.record_on(
            &path,
            "Tetris DX",
            Some("KEY-DX".to_owned()),
            Some(ConsoleKind::Gbc),
        );
        let ctx = Context::default();
        let (_, texts) = recent_frame(&ctx, &mut app, Vec::new());
        let row = texts
            .iter()
            .find(|(text, _)| text.ends_with("Tetris DX"))
            .map(|(_, rect)| rect.center())
            .expect("the Recent menu shows Tetris DX");
        let press = |pressed| egui::Event::PointerButton {
            pos: row,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let mut picks = Vec::new();
        for events in [
            vec![egui::Event::PointerMoved(row)],
            vec![press(true)],
            vec![press(false)],
        ] {
            picks.push(recent_frame(&ctx, &mut app, events).0);
        }
        assert_eq!(picks, [None, None, Some((path, None))]);
        assert_eq!(
            app.ui.settings.console_for("KEY-DX", RomCgbMode::CgbAndGb),
            GbModel::Cgb
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_recent_row_names_its_console_on_hover() {
        use citrine_gb::console::ConsoleKind;
        let mut recent = crate::app::ui_state::recent::RecentRoms::default();
        let path = std::path::Path::new("tetris.gbc");
        recent.record_on(path, "Tetris", None, Some(ConsoleKind::Gbc));
        recent.record_on(std::path::Path::new("mystery.gb"), "Mystery", None, None);
        let hints: Vec<String> = recent.entries().iter().map(recent_hint).collect();
        assert_eq!(hints, ["mystery.gb", "Game Boy Color\ntetris.gbc"]);
    }
}

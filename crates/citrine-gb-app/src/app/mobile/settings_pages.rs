use super::kit::{self, Row};
use super::sheet::{self, Amount, SheetResponse};
use crate::app::Citrine;
use crate::app::big_screen;
use crate::app::system_files::{FileChoice, SystemFile};
use crate::app::tabs::Tab;
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::ui_state::settings_tree::{GroupId, SettingsTab, Surface};
use crate::app::widgets::settings::page::Page;
use crate::app::widgets::settings::scope::View;
use egui::{Context, Id, Ui};
use std::fmt::Display;
use strum::IntoEnumIterator;

mod about;
mod app;
mod capture;
pub(super) mod cartridge;
mod controls;
#[cfg(feature = "nds")]
mod ds;
#[cfg(feature = "nds")]
mod ds_screens;
pub(super) mod game_boy;
mod gba;
mod keys;
mod online;
mod playing;
pub(super) mod scope;
mod screen;
mod sound;
mod storage;
mod sync;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeysPart {
    Buttons,
    Shortcuts,
    Split,
    Tilt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sheet {
    ButtonSize,
    Opacity,
    UiSize,
    TouchScaling,
    Landscape,
    HiddenButtons,
    Keys(KeysPart),
    DeadZone,
    Volume,
    Latency,
    Slowdown,
    #[cfg(not(target_arch = "wasm32"))]
    Device,
    #[cfg(feature = "nds")]
    Microphone,
    Console,
    GameConsole,
    File(crate::app::system_files::SystemFile),
    GbaScreen,
    #[cfg(feature = "nds")]
    DsNickname,
    #[cfg(feature = "nds")]
    DsBirthday,
    #[cfg(feature = "nds")]
    DsBirthdayDay,
    #[cfg(feature = "nds")]
    DsLanguage,
    #[cfg(feature = "nds")]
    DsColor,
    #[cfg(feature = "nds")]
    DsClock,
    #[cfg(ds_wifi)]
    DsWifi,
    #[cfg(ds_wifi)]
    DsDns,
    Layout,
    Theme,
    DiscordEvery,
    DiscordScreens,
    AutosaveEvery,
    AutosaveKeep,
    FastForward,
    FastForwardKey,
    FastForwardSound,
    QuickLoad,
    InBackground,
    Subfolders,
    Rewind,
    ScreenshotScale,
    RecordingScale,
    RenderVideo,
    ClipLength,
    Clock,
    Sunlight,
    Camera,
    Palette,
    OwnPalettes,
    EdgeDarkness,
    CornerDarkness,
    GhostStrength,
    HudCard,
    GbcScreen,
    #[cfg(feature = "nds")]
    DsCorner,
    #[cfg(feature = "nds")]
    DsSmall,
    #[cfg(feature = "nds")]
    DsGap,
    #[cfg(feature = "nds")]
    DsRotation,
    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    Ds3dResolution,
    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    DsSmoothEdges,
    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    DsTextureFilter,
    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    DsDistantTextures,
    PasteCode,
    DeletePalette,
    InputDelay,
    TypingText,
    KeepSaves,
    ConfirmButton,
    ButtonIcons,
    Backdrop,
}

fn sheet_id() -> Id {
    Id::new("settings_sheet")
}

fn label(row: RowId) -> &'static str {
    row.label(Surface::Phone)
}

impl Citrine {
    fn file_row(&self, list: &mut kit::List<'_>, file: SystemFile) -> Option<Sheet> {
        let value = FileChoice::current(file, self.emulator.system_file_name(file)).to_string();
        opens(
            list,
            Row::new(label(file.row())).value(value),
            Sheet::File(file),
        )
    }

    pub(in crate::app::mobile) fn file_sheet(&mut self, ctx: &Context, file: SystemFile) -> bool {
        let name = self.emulator.system_file_name(file);
        let current = FileChoice::current(file, name);
        let shown = sheet::show_with_info(ctx, sheet_id(), file.noun(), Some(file.hint()), |ui| {
            sheet::choices(ui, FileChoice::all(file, name), current, |choice| {
                choice.to_string()
            })
        });
        match shown.inner {
            Some(picked) => {
                if picked != current {
                    picked.apply(file, &mut self.events);
                }
                true
            }
            None => shown.closed,
        }
    }
}

fn opens(list: &mut kit::List<'_>, row: Row<'_>, sheet: Sheet) -> Option<Sheet> {
    list.row(row).clicked().then_some(sheet)
}

fn stepped<T: IntoEnumIterator + PartialEq + Copy>(
    list: &mut kit::List<'_>,
    row: Row<'_>,
    value: &mut T,
    dirty: &mut bool,
    sheet: Sheet,
) -> Option<Sheet> {
    let response = list.row(row.stepper());
    if let Some(step) = big_screen::take_step(&response) {
        *value = big_screen::cycle(*value, step);
        *dirty = true;
    }
    response.clicked().then_some(sheet)
}

fn pick<T: IntoEnumIterator + Copy + PartialEq + Display>(
    ctx: &Context,
    title: &str,
    note: Option<&str>,
    value: &mut T,
) -> (bool, bool) {
    pick_from(ctx, title, note, T::iter(), value, |v| v.to_string())
}

fn pick_from<T: Copy + PartialEq>(
    ctx: &Context,
    title: &str,
    note: Option<&str>,
    options: impl IntoIterator<Item = T>,
    value: &mut T,
    label: impl Fn(T) -> String,
) -> (bool, bool) {
    let current = *value;
    let shown = sheet::show(ctx, sheet_id(), title, |ui| {
        if let Some(note) = note {
            sheet::note(ui, note);
        }
        sheet::choices(ui, options, current, label)
    });
    match shown.inner {
        Some(picked) => {
            *value = picked;
            (picked != current, true)
        }
        None => (false, shown.closed),
    }
}

fn slide<T: egui::emath::Numeric>(
    ctx: &Context,
    title: &str,
    note: Option<&str>,
    amount: Amount<'_, T>,
) -> SheetResponse<egui::Response> {
    sheet::show(ctx, sheet_id(), title, |ui| {
        if let Some(note) = note {
            sheet::note(ui, note);
        }
        sheet::amount(ui, amount)
    })
}

fn settled(response: &egui::Response) -> bool {
    response.drag_stopped() || (response.changed() && !response.dragged())
}

impl Citrine {
    pub(super) fn phone_settings_page(&mut self, ui: &mut Ui, page: SettingsTab) {
        self.big.focused_own = None;
        if let Some(row) = self.ui.settings.find.take() {
            crate::app::widgets::settings::spotlight::aim(ui.ctx(), row.label(Surface::Phone));
        }
        if let Some(group) = self.ui.settings.find_group.take() {
            crate::app::widgets::settings::spotlight::aim_heading(ui.ctx(), group.title());
        }
        let frame = crate::app::widgets::settings::scope::begin(
            &mut self.ui.settings,
            page,
            Surface::Phone,
        );
        match page {
            SettingsTab::Screen => kit::place(ui, "screen_page", |ui| self.screen_page(ui)),
            SettingsTab::Sound => kit::place(ui, "sound_page", |ui| self.sound_page(ui)),
            SettingsTab::Controls => kit::place(ui, "controls_page", |ui| self.controls_page(ui)),
            SettingsTab::Playing => kit::place(ui, "playing_page", |ui| self.playing_page(ui)),
            SettingsTab::Capture => kit::place(ui, "capture_page", |ui| self.capture_page(ui)),
            SettingsTab::GameBoy => kit::place(ui, "game_boy_page", |ui| self.game_boy_page(ui)),
            SettingsTab::Gba => kit::place(ui, "gba_page", |ui| self.gba_page(ui)),
            #[cfg(feature = "nds")]
            SettingsTab::Ds => kit::place(ui, "ds_page", |ui| self.ds_page(ui)),
            #[cfg(not(feature = "nds"))]
            SettingsTab::Ds => {}
            SettingsTab::App => kit::place(ui, "app_page", |ui| self.app_page(ui)),
            SettingsTab::Online => kit::place(ui, "online_page", |ui| self.online_page(ui)),
            SettingsTab::Sync => kit::place(ui, "sync_page", |ui| self.sync_page(ui)),
            SettingsTab::Storage => kit::place(ui, "storage_page", |ui| self.storage_page(ui)),
            SettingsTab::About => self.about_page(ui),
            SettingsTab::Developer => {
                kit::place(ui, "developer_page", |ui| self.developer_page(ui))
            }
        }
        self.settings_sheet(&ui.ctx().clone());
        self.ui.settings.end_scope(frame);
    }

    pub(super) fn settings_sheet(&mut self, ctx: &Context) {
        if let Some(open) = self.mobile.sheet {
            let closed = match open {
                Sheet::ButtonSize
                | Sheet::Opacity
                | Sheet::TouchScaling
                | Sheet::Landscape
                | Sheet::HiddenButtons
                | Sheet::ConfirmButton
                | Sheet::ButtonIcons
                | Sheet::Backdrop
                | Sheet::Keys(_)
                | Sheet::DeadZone => self.controls_sheet(ctx, open),
                Sheet::Volume | Sheet::Latency | Sheet::Slowdown | Sheet::FastForwardSound => {
                    self.sound_sheet(ctx, open)
                }
                #[cfg(not(target_arch = "wasm32"))]
                Sheet::Device => self.sound_sheet(ctx, open),
                Sheet::Console
                | Sheet::GameConsole
                | Sheet::Palette
                | Sheet::OwnPalettes
                | Sheet::PasteCode
                | Sheet::DeletePalette
                | Sheet::GbcScreen => self.game_boy_sheet(ctx, open),
                Sheet::GbaScreen => self.gba_sheet(ctx),
                Sheet::File(file) => self.file_sheet(ctx, file),
                #[cfg(feature = "nds")]
                Sheet::DsNickname
                | Sheet::DsBirthday
                | Sheet::DsBirthdayDay
                | Sheet::DsLanguage
                | Sheet::DsColor
                | Sheet::DsClock
                | Sheet::DsCorner
                | Sheet::DsSmall
                | Sheet::DsGap
                | Sheet::DsRotation => self.ds_sheet(ctx, open),
                #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
                Sheet::Ds3dResolution
                | Sheet::DsSmoothEdges
                | Sheet::DsTextureFilter
                | Sheet::DsDistantTextures => self.ds_sheet(ctx, open),
                #[cfg(ds_wifi)]
                Sheet::DsWifi | Sheet::DsDns => self.ds_sheet(ctx, open),
                #[cfg(feature = "nds")]
                Sheet::Microphone => self.ds_sheet(ctx, open),
                Sheet::Layout
                | Sheet::Theme
                | Sheet::UiSize
                | Sheet::DiscordEvery
                | Sheet::DiscordScreens
                | Sheet::Subfolders => self.app_sheet(ctx, open),
                Sheet::AutosaveEvery
                | Sheet::AutosaveKeep
                | Sheet::FastForward
                | Sheet::FastForwardKey
                | Sheet::QuickLoad
                | Sheet::InBackground
                | Sheet::Rewind => self.playing_sheet(ctx, open),
                Sheet::ScreenshotScale
                | Sheet::RecordingScale
                | Sheet::RenderVideo
                | Sheet::ClipLength => self.capture_sheet(ctx, open),
                Sheet::Clock | Sheet::Sunlight | Sheet::Camera => self.cartridge_sheet(ctx, open),
                Sheet::EdgeDarkness
                | Sheet::CornerDarkness
                | Sheet::GhostStrength
                | Sheet::HudCard => self.screen_sheet(ctx, open),
                Sheet::InputDelay | Sheet::TypingText => self.online_sheet(ctx, open),
                Sheet::KeepSaves => self.sync_sheet(ctx),
            };
            if closed {
                self.mobile.sheet = None;
            }
        }
    }

    fn tree_page(
        &mut self,
        ui: &mut Ui,
        tab: SettingsTab,
        hint: impl Fn(GroupId) -> Option<&'static str>,
        mut row: impl FnMut(&mut Self, &mut kit::List<'_>, RowId) -> Option<Sheet>,
        foot: impl FnOnce(&mut Page),
    ) {
        let dev_mode = self.ui.settings.dev_mode;
        let view = View::of(&self.ui.settings, tab, Surface::Phone);
        let big = self.big_screen_on();
        let chips = scope::Chips::new(big);
        let mut open = None;
        let game = crate::app::widgets::settings::scope::game_title(&self.ui.settings).to_owned();
        let mut page = Page::begin(ui);
        if view.bar {
            let mut current = self.ui.settings.scope();
            let shoulders = big.then(|| self.shown_family());
            page.plain(|ui| scope::segments(ui, &mut current, &game, shoulders));
            self.ui.settings.set_scope(current);
        }
        let tagged: Vec<RowId> = tab
            .groups_on(Surface::Phone, dev_mode)
            .flat_map(|group| group.rows_on(Surface::Phone, dev_mode))
            .filter(|id| view.tag(&self.ui.settings, *id).is_some())
            .collect();
        let mut focused_own = None;
        for group in tab
            .groups_on(Surface::Phone, dev_mode)
            .filter(|group| view.group(*group))
        {
            page.tree_group(group, hint(group), |ui| {
                kit::list(ui, |list| {
                    let column = scope::chip_column(list.ui(), &tagged);
                    for id in group
                        .rows_on(Surface::Phone, dev_mode)
                        .filter(|id| view.row(*id))
                    {
                        let top = list.ui().min_rect().bottom();
                        if let Some(sheet) = row(self, list, id) {
                            open = Some(sheet);
                        }
                        let Some(tag) = view.tag(&self.ui.settings, id) else {
                            continue;
                        };
                        if scope::mark(list.ui(), id, &tag, top, column, &chips) {
                            self.ui.settings.drop_later(id, Surface::Phone);
                        }
                        if tag.drops() && scope::holds_focus(list.ui(), top) {
                            focused_own = Some(id);
                        }
                    }
                })
            });
        }
        self.big.focused_own = focused_own;
        foot(&mut page);
        page.end();
        self.open_sheet(open);
    }

    fn developer_page(&mut self, ui: &mut Ui) {
        self.tree_page(
            ui,
            SettingsTab::Developer,
            |_| None,
            |app, list, row| {
                match row {
                    RowId::RandomRam => {
                        let s = &mut app.ui.settings;
                        s.dirty |=
                            list.switch(kit::SwitchRow::new(label(row), &mut s.randomized_ram));
                    }
                    RowId::DsFiles => {
                        let entry = Row::new(label(row)).icon(crate::icons::FOLDER_OPEN);
                        if list.row(entry).clicked() {
                            app.show_tab(Tab::DsFiles, true);
                        }
                    }
                    _ => {}
                }
                None
            },
            |_| {},
        );
    }

    pub(super) fn open_sheet(&mut self, sheet: Option<Sheet>) {
        if sheet.is_some() {
            self.mobile.sheet = sheet;
            self.release_touch();
        }
    }
}

use super::kit;
use super::sheet::{self, Amount, SheetResponse};
use crate::app::Citrine;
use crate::app::tabs::Tab;
use crate::app::ui_state::settings::SettingsTab;
use crate::app::widgets::settings::page::Page;
use egui::{Context, Id, Ui};
use std::fmt::Display;
use strum::IntoEnumIterator;

mod about;
mod cartridge;
mod controls;
#[cfg(feature = "nds")]
mod ds;
mod ds_screens;
mod general;
mod online;
mod sound;
mod style;
mod your_data;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sheet {
    ButtonSize,
    Opacity,
    UiSize,
    TouchScaling,
    Landscape,
    HiddenButtons,
    Keys,
    Volume,
    Latency,
    Slowdown,
    Channels,
    #[cfg(not(target_arch = "wasm32"))]
    Device,
    Console,
    GameConsole,
    #[cfg(feature = "gba")]
    GbaBios,
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
    #[cfg(feature = "nds")]
    DsFile(crate::app::ds_firmware::DsFile),
    #[cfg(ds_wifi)]
    DsWifi,
    #[cfg(ds_wifi)]
    DsDns,
    Layout,
    Theme,
    AutosaveEvery,
    AutosaveKeep,
    FastForward,
    Rewind,
    ScreenshotScale,
    RecordingScale,
    Clock,
    Sunlight,
    Camera,
    Palette,
    GamePalette,
    EdgeDarkness,
    CornerDarkness,
    GhostStrength,
    HudCard,
    GbcScreen,
    DsLayout,
    DsCorner,
    DsSmall,
    DsGap,
    DsRotation,
    #[cfg(not(target_arch = "wasm32"))]
    Ds3dResolution,
    #[cfg(not(target_arch = "wasm32"))]
    DsSmoothEdges,
    PasteCode,
    DeletePalette,
    InputDelay,
    ConfirmButton,
    ButtonIcons,
    Backdrop,
}

fn sheet_id() -> Id {
    Id::new("settings_sheet")
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
        match page {
            SettingsTab::Controls => kit::place(ui, "controls_page", |ui| self.controls_page(ui)),
            SettingsTab::Sound => kit::place(ui, "sound_page", |ui| self.sound_page(ui)),
            SettingsTab::General => kit::place(ui, "general_page", |ui| self.general_page(ui)),
            SettingsTab::Style => kit::place(ui, "style_page", |ui| self.style_page(ui)),
            SettingsTab::Online => kit::place(ui, "online_page", |ui| self.online_page(ui)),
            SettingsTab::YourData => kit::place(ui, "your_data_page", |ui| self.your_data_page(ui)),
            SettingsTab::Developer => {
                kit::place(ui, "developer_page", |ui| self.developer_page(ui))
            }
        }
        if let Some(open) = self.mobile.sheet {
            let ctx = ui.ctx().clone();
            let closed = match open {
                Sheet::ButtonSize
                | Sheet::Opacity
                | Sheet::UiSize
                | Sheet::TouchScaling
                | Sheet::Landscape
                | Sheet::HiddenButtons
                | Sheet::ConfirmButton
                | Sheet::ButtonIcons
                | Sheet::Backdrop
                | Sheet::Keys => self.controls_sheet(&ctx, open),
                Sheet::Volume | Sheet::Latency | Sheet::Slowdown | Sheet::Channels => {
                    self.sound_sheet(&ctx, open)
                }
                #[cfg(not(target_arch = "wasm32"))]
                Sheet::Device => self.sound_sheet(&ctx, open),
                Sheet::Console
                | Sheet::GameConsole
                | Sheet::Layout
                | Sheet::Theme
                | Sheet::AutosaveEvery
                | Sheet::AutosaveKeep
                | Sheet::FastForward
                | Sheet::Rewind
                | Sheet::ScreenshotScale
                | Sheet::RecordingScale => self.general_sheet(&ctx, open),
                #[cfg(feature = "gba")]
                Sheet::GbaBios => self.general_sheet(&ctx, open),
                #[cfg(feature = "nds")]
                Sheet::DsNickname
                | Sheet::DsBirthday
                | Sheet::DsBirthdayDay
                | Sheet::DsLanguage
                | Sheet::DsColor
                | Sheet::DsClock
                | Sheet::DsFile(_) => self.ds_sheet(&ctx, open),
                #[cfg(ds_wifi)]
                Sheet::DsWifi | Sheet::DsDns => self.ds_sheet(&ctx, open),
                Sheet::Clock | Sheet::Sunlight | Sheet::Camera => self.cartridge_sheet(&ctx, open),
                Sheet::Palette
                | Sheet::GamePalette
                | Sheet::EdgeDarkness
                | Sheet::CornerDarkness
                | Sheet::GhostStrength
                | Sheet::HudCard
                | Sheet::GbcScreen
                | Sheet::DsLayout
                | Sheet::DsCorner
                | Sheet::DsSmall
                | Sheet::DsGap
                | Sheet::DsRotation
                | Sheet::PasteCode
                | Sheet::DeletePalette => self.style_sheet(&ctx, open),
                #[cfg(not(target_arch = "wasm32"))]
                Sheet::Ds3dResolution | Sheet::DsSmoothEdges => self.style_sheet(&ctx, open),
                Sheet::InputDelay => self.online_sheet(&ctx),
            };
            if closed {
                self.mobile.sheet = None;
            }
        }
    }

    fn developer_page(&mut self, ui: &mut Ui) {
        let s = &mut self.ui.settings;
        let mut page = Page::begin(ui);
        page.plain(|ui| {
            kit::list(ui, |list| {
                s.dirty |= list.switch(kit::SwitchRow::new(
                    "Randomized power-on RAM",
                    &mut s.randomized_ram,
                ));
            })
        });
        let files = page.plain(|ui| {
            kit::list(ui, |list| {
                list.row(kit::Row::new(Tab::DsFiles.title()).icon(crate::icons::FOLDER_OPEN))
                    .clicked()
            })
        });
        page.end();
        if files {
            self.show_tab(Tab::DsFiles, true);
        }
    }

    fn open_sheet(&mut self, sheet: Option<Sheet>) {
        if sheet.is_some() {
            self.mobile.sheet = sheet;
            self.release_touch();
        }
    }
}

use super::kit;
use super::sheet::{self, Amount, SheetResponse};
use crate::app::Citrine;
use crate::app::ui_state::settings::SettingsTab;
use egui::{Context, Id, Ui};
use std::fmt::Display;
use strum::IntoEnumIterator;

mod about;
mod controls;
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
    Channels,
    #[cfg(not(target_arch = "wasm32"))]
    Device,
    Console,
    GameConsole,
    Layout,
    Theme,
    AutosaveEvery,
    AutosaveKeep,
    ScreenshotScale,
    RecordingScale,
    Palette,
    GamePalette,
    EdgeDarkness,
    CornerDarkness,
    GhostStrength,
    GbcScreen,
    PasteCode,
    DeletePalette,
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
            _ => {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(kit::SIDE as i8, 0))
                    .show(ui, |ui| {
                        let mut viewer = self.viewer();
                        crate::app::tabs::settings::content(&mut viewer, ui, 0.0);
                    });
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
                | Sheet::Keys => self.controls_sheet(&ctx, open),
                Sheet::Volume | Sheet::Latency | Sheet::Channels => self.sound_sheet(&ctx, open),
                #[cfg(not(target_arch = "wasm32"))]
                Sheet::Device => self.sound_sheet(&ctx, open),
                Sheet::Console
                | Sheet::GameConsole
                | Sheet::Layout
                | Sheet::Theme
                | Sheet::AutosaveEvery
                | Sheet::AutosaveKeep
                | Sheet::ScreenshotScale
                | Sheet::RecordingScale => self.general_sheet(&ctx, open),
                Sheet::Palette
                | Sheet::GamePalette
                | Sheet::EdgeDarkness
                | Sheet::CornerDarkness
                | Sheet::GhostStrength
                | Sheet::GbcScreen
                | Sheet::PasteCode
                | Sheet::DeletePalette => self.style_sheet(&ctx, open),
            };
            if closed {
                self.mobile.sheet = None;
            }
        }
    }

    fn open_sheet(&mut self, sheet: Option<Sheet>) {
        if sheet.is_some() {
            self.mobile.sheet = sheet;
            self.release_touch();
        }
    }
}

#[cfg(test)]
mod tests;

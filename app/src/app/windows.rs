use crate::{icons, Citrine};
use bitflags::{bitflags, bitflags_match, Flags};
use egui::{Context, Id, Ui, Widget, WidgetText};

mod debug_actions;
mod e2e_test;
mod rom_info;
mod settings;
mod time_control;

bitflags! {
    #[derive(Default, Copy, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
    pub struct ActiveWindows: u16 {
        const TIME_CONTROL = 0b0000_0000_0000_0001;
        const ROM_INFO = 0b0000_0000_0000_0010;
        const DEBUG_ACTIONS = 0b0000_0000_0000_0100;
        const SETTINGS = 0b0000_0000_0000_1000;
        const E2E_TEST = 0b0000_0000_0001_0000;
    }
}

impl ActiveWindows {
    const ORDER: &'static [ActiveWindows] = &[
        ActiveWindows::SETTINGS,
        ActiveWindows::ROM_INFO,
        ActiveWindows::TIME_CONTROL,
        ActiveWindows::DEBUG_ACTIONS,
        ActiveWindows::E2E_TEST,
    ];

    pub fn show_all(&self, ctx: &Context, app: &mut Citrine) {
        self.iter().for_each(|id| id.show(ctx, app));
    }

    pub fn show(&self, ctx: &Context, app: &mut Citrine) {
        bitflags_match!(*self, {
            Self::TIME_CONTROL => time_control::TimeControlWindow::new().show(ctx, app),
            Self::ROM_INFO => rom_info::RomInfoWindow::new().show(ctx, app),
            Self::DEBUG_ACTIONS => debug_actions::DebugActionsWindow::new().show(ctx, app),
            Self::SETTINGS => settings::SettingsWindow::new().show(ctx, app),
            Self::E2E_TEST => e2e_test::E2ETestWindow::new().show(ctx, app),
            _ => {}
        });
    }

    pub fn label(&self) -> &'static str {
        bitflags_match!(*self, {
            Self::TIME_CONTROL => "Time Control",
            Self::ROM_INFO => "Rom Info",
            Self::DEBUG_ACTIONS => "Debug Actions",
            Self::SETTINGS => "Settings",
            Self::E2E_TEST => "E2E Test",
            _ => ""
        })
    }

    pub fn toggle_menu(&mut self, ui: &mut Ui) {
        ui.menu_button(icons::BROWSERS, |ui| {
            Self::ORDER.iter().for_each(|id| {
                let mut is_open = self.contains(*id);
                let response = ui.checkbox(&mut is_open, id.label());
                if response.clicked() {
                    self.toggle(*id);
                }
            });
        });
    }
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct Windows {
    pub active: ActiveWindows,
}

impl Windows {
    pub fn is_active(&self, window: ActiveWindows) -> bool {
        self.active.contains(window)
    }

    pub fn set_active(&mut self, window: ActiveWindows, active: bool) {
        if active {
            self.active.insert(window);
        } else {
            self.active.remove(window);
        }
    }
}

pub trait AppWindow: Sized {
    const ID: ActiveWindows;

    fn title(_app: &mut Citrine) -> impl Into<WidgetText>;

    fn resizable(_app: &mut Citrine) -> bool {
        true
    }

    fn movable(_app: &mut Citrine) -> bool {
        true
    }

    fn collapsible(_app: &mut Citrine) -> bool {
        true
    }

    fn ui(&mut self, ui: &mut Ui, app: &mut Citrine);

    fn show(mut self, ctx: &Context, app: &mut Citrine) {
        let mut is_open = app.ui.windows.is_active(Self::ID);
        egui::Window::new(Self::title(app))
            .id(Id::new(Self::ID))
            .open(&mut is_open)
            .resizable(Self::resizable(app))
            .movable(Self::movable(app))
            .collapsible(Self::collapsible(app))
            .show(ctx, |ui| self.ui(ui, app));
        app.ui.windows.set_active(Self::ID, is_open);
    }
}

use crate::app::panels::Panels;
use crate::app::settings::Settings;
use crate::app::widgets::registers::RegistersState;
use crate::app::widgets::time_control::TimeControlState;
use crate::app::windows::Windows;

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct UiState {
    pub e2e: E2E,
    pub panels: Panels,
    pub registers: RegistersState,
    pub settings: Settings,
    pub time_ctrl: TimeControlState,
    pub windows: Windows,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct E2E {
    pub title: String,
    pub description: String,
}

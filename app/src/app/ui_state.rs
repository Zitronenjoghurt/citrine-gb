use crate::app::widgets::registers::RegistersState;
use crate::app::widgets::time_control::TimeControlState;
use settings::Settings;

pub mod settings;

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct UiState {
    pub e2e: E2E,
    pub registers: RegistersState,
    pub settings: Settings,
    pub time_ctrl: TimeControlState,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct E2E {
    pub title: String,
    pub description: String,
}

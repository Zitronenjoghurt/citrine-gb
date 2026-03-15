use crate::app::widgets::info::InfoState;
use crate::app::widgets::registers::RegistersState;
use crate::app::widgets::time_control::TimeControlState;
use crate::utils::avg_timer::AvgTimer;
use settings::Settings;

pub mod settings;
pub mod ui_theme;

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct UiState {
    pub e2e: E2E,
    pub info: InfoState,
    pub registers: RegistersState,
    pub settings: Settings,
    pub time_ctrl: TimeControlState,
    #[serde(skip, default)]
    pub update_avg_timer: AvgTimer,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct E2E {
    pub title: String,
    pub description: String,
}

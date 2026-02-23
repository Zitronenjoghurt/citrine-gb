use crate::app::panels::Panels;
use crate::app::widgets::registers::RegistersState;
use crate::app::widgets::time_control::TimeControlState;
use crate::app::windows::Windows;

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct UiState {
    pub panels: Panels,
    pub registers: RegistersState,
    pub time_ctrl: TimeControlState,
    pub windows: Windows,
}

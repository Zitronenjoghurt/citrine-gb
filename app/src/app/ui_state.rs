use crate::app::panels::Panels;
use crate::app::widgets::registers::RegistersState;

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct UiState {
    pub panels: Panels,
    pub registers: RegistersState,
}

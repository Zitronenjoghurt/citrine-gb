#[derive(Debug, Copy, Clone, serde::Serialize, serde::Deserialize)]
pub enum PanelKind {
    Debug,
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Panels {
    pub left: Option<PanelKind>,
    pub right: Option<PanelKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct WalkerSteps {
    pub in_game: bool,
    pub phone: bool,
}

impl Default for WalkerSteps {
    fn default() -> Self {
        Self {
            in_game: false,
            phone: true,
        }
    }
}

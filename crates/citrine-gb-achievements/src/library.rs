use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameProgress {
    pub game_id: u32,
    pub total: u32,
    pub unlocked: u32,
    pub unlocked_hardcore: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetInfo {
    pub game_id: u32,
    pub title: String,
    pub total: u32,
    pub points: u32,
    pub hashes: Vec<String>,
}

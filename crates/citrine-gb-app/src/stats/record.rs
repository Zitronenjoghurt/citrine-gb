use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameDay {
    pub play_ms: u64,
    pub sessions: u32,
    pub launches: u32,
    pub snapshots: u32,
    pub loads: u32,
    pub fast_forward_ms: u64,
    pub rewinds: u32,
    pub link_ms: u64,
    pub together_ms: u64,
}

impl GameDay {
    fn merge(&mut self, other: &GameDay) {
        self.play_ms = self.play_ms.max(other.play_ms);
        self.sessions = self.sessions.max(other.sessions);
        self.launches = self.launches.max(other.launches);
        self.snapshots = self.snapshots.max(other.snapshots);
        self.loads = self.loads.max(other.loads);
        self.fast_forward_ms = self.fast_forward_ms.max(other.fast_forward_ms);
        self.rewinds = self.rewinds.max(other.rewinds);
        self.link_ms = self.link_ms.max(other.link_ms);
        self.together_ms = self.together_ms.max(other.together_ms);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Progress {
    pub in_game_secs: u64,
    pub dex_owned: u16,
    pub dex_seen: u16,
    pub badges: u8,
}

impl Progress {
    pub fn merge(&mut self, other: &Progress) {
        self.in_game_secs = self.in_game_secs.max(other.in_game_secs);
        self.dex_owned = self.dex_owned.max(other.dex_owned);
        self.dex_seen = self.dex_seen.max(other.dex_seen);
        self.badges = self.badges.max(other.badges);
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameRecord {
    pub name: String,
    pub first_played: u64,
    pub last_played: u64,
    pub longest_session_ms: u64,
    pub days: BTreeMap<String, GameDay>,
    pub progress: BTreeMap<String, Progress>,
}

impl GameRecord {
    fn merge(&mut self, other: &GameRecord) {
        if !other.name.is_empty() && (self.name.is_empty() || other.last_played > self.last_played)
        {
            self.name = other.name.clone();
        }
        if other.first_played > 0
            && (self.first_played == 0 || other.first_played < self.first_played)
        {
            self.first_played = other.first_played;
        }
        self.last_played = self.last_played.max(other.last_played);
        self.longest_session_ms = self.longest_session_ms.max(other.longest_session_ms);
        for (day, counts) in &other.days {
            self.days.entry(day.clone()).or_default().merge(counts);
        }
        for (day, progress) in &other.progress {
            self.progress
                .entry(day.clone())
                .or_default()
                .merge(progress);
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppDay {
    pub chat_messages: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Record {
    pub games: BTreeMap<String, GameRecord>,
    pub days: BTreeMap<String, AppDay>,
}

impl Record {
    pub fn merge(&mut self, other: &Record) {
        for (rom, game) in &other.games {
            self.games.entry(rom.clone()).or_default().merge(game);
        }
        for (day, app) in &other.days {
            let mine = self.days.entry(day.clone()).or_default();
            mine.chat_messages = mine.chat_messages.max(app.chat_messages);
        }
    }

    pub fn game(&mut self, rom: &str) -> &mut GameRecord {
        self.games.entry(rom.to_string()).or_default()
    }

    pub fn game_day(&mut self, rom: &str, day: &str) -> &mut GameDay {
        self.game(rom).days.entry(day.to_string()).or_default()
    }

    pub fn app_day(&mut self, day: &str) -> &mut AppDay {
        self.days.entry(day.to_string()).or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.games.is_empty() && self.days.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Book {
    pub device: String,
    pub devices: BTreeMap<String, Record>,
}

impl Book {
    pub fn new() -> Self {
        Self {
            device: crate::storage::cloud::random_token(),
            devices: BTreeMap::new(),
        }
    }

    pub fn own(&mut self) -> &mut Record {
        if self.device.is_empty() {
            self.device = crate::storage::cloud::random_token();
        }
        self.devices.entry(self.device.clone()).or_default()
    }

    pub fn own_record(&self) -> Option<&Record> {
        self.devices.get(&self.device)
    }

    pub fn merge_device(&mut self, device: &str, record: &Record) {
        if device.is_empty() {
            return;
        }
        self.devices
            .entry(device.to_string())
            .or_default()
            .merge(record);
    }

    pub fn merge(&mut self, other: &Book) {
        for (device, record) in &other.devices {
            self.merge_device(device, record);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.devices.values().all(Record::is_empty)
    }
}

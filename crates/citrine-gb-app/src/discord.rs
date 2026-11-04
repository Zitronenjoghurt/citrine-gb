#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
mod rpc;

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub use rpc::Rpc;
use serde::{Deserialize, Serialize};
use strum_macros::EnumIter;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, EnumIter)]
pub enum ShotInterval {
    Minute,
    #[default]
    TwoMinutes,
    FiveMinutes,
}

impl ShotInterval {
    pub fn seconds(self) -> u64 {
        match self {
            Self::Minute => 60,
            Self::TwoMinutes => 120,
            Self::FiveMinutes => 300,
        }
    }
}

impl std::fmt::Display for ShotInterval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} min", self.seconds() / 60)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiscordSettings {
    pub enabled: bool,
    pub game: bool,
    pub details: bool,
    pub play_time: bool,
    pub screenshot: bool,
    pub every: ShotInterval,
}

impl Default for DiscordSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            game: true,
            details: true,
            play_time: true,
            screenshot: false,
            every: ShotInterval::default(),
        }
    }
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameNow {
    pub name: String,
    pub details: Option<String>,
    pub hover: Option<String>,
    pub started_at: u64,
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Presence {
    pub details: String,
    pub state: Option<String>,
    pub started_at: Option<u64>,
    pub screenshot: bool,
    pub image: Option<String>,
    pub hover: Option<String>,
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
#[derive(Debug, Clone, Default)]
pub struct Shots {
    game: Option<String>,
    checked: Option<std::time::Instant>,
    frame: Option<u64>,
    running_since: Option<std::time::Instant>,
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
const FIRST_SHOT_AFTER: u64 = 10;

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shot {
    Wait,
    Take,
    Forget,
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub struct Unit<'a> {
    pub game: String,
    pub running: bool,
    pub frame: &'a [u8],
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub fn my_unit<'a>(
    session: Option<&'a crate::play::PlaySession>,
    solo: Option<Unit<'a>>,
) -> Option<Unit<'a>> {
    match session.filter(|s| s.is_live()) {
        Some(s) if s.runs_engine() => s.console(true).map(|console| Unit {
            game: format!(
                "together {}",
                citrine_gb::console::Console::rom_id(console).sha256_hex_string()
            ),
            running: true,
            frame: citrine_gb::console::Console::frame(console),
        }),
        Some(s) if s.holds_my_game() || s.watching_only() => None,
        _ => solo,
    }
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
impl Shots {
    pub fn due(
        &mut self,
        now: std::time::Instant,
        every: ShotInterval,
        unit: Option<(&str, bool)>,
        frame: impl FnOnce() -> u64,
    ) -> Shot {
        let game = unit.map(|(game, _)| game);
        if self.game.as_deref() != game {
            *self = Self {
                game: game.map(str::to_string),
                ..Self::default()
            };
            return Shot::Forget;
        }
        if !unit.is_some_and(|(_, running)| running) {
            self.running_since = None;
            return Shot::Wait;
        }
        let since = *self.running_since.get_or_insert(now);
        if now.duration_since(since).as_secs() < FIRST_SHOT_AFTER
            || self
                .checked
                .is_some_and(|at| now.duration_since(at).as_secs() < every.seconds())
        {
            return Shot::Wait;
        }
        self.checked = Some(now);
        let frame = frame();
        if self.frame == Some(frame) {
            return Shot::Wait;
        }
        self.frame = Some(frame);
        Shot::Take
    }
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub const SHOT_SCALE: usize = 3;

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub fn frame_key(pixels: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    pixels.hash(&mut hasher);
    hasher.finish()
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub fn screen_url(address: &crate::online::ServerAddress, path: &str, version: u64) -> String {
    address.web_url(&format!("{path}?v={version}"))
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub fn game_now(
    emulator: &crate::emulator::Emulator,
    stats: &crate::stats::Stats,
    info: &crate::game_info::GameInfo,
) -> Option<GameNow> {
    let rom = emulator.rom_key()?;
    Some(GameNow {
        name: emulator.game_name().to_string(),
        details: info.summary(),
        hover: info.lead_and_money(),
        started_at: stats
            .started_at(rom)
            .unwrap_or_else(crate::storage::now_unix),
    })
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub fn presence(
    settings: &DiscordSettings,
    game: Option<&GameNow>,
    image: Option<String>,
) -> Option<Presence> {
    if !settings.enabled {
        return None;
    }
    let Some(game) = game else {
        return Some(Presence {
            details: "Choosing a game".to_string(),
            ..Default::default()
        });
    };
    let details = if settings.game {
        game.name.clone()
    } else {
        "Playing a game".to_string()
    };
    Some(Presence {
        details,
        state: game.details.clone().filter(|_| settings.details),
        hover: game.hover.clone().filter(|_| settings.details),
        started_at: settings.play_time.then_some(game.started_at),
        screenshot: settings.screenshot,
        image: image.filter(|_| settings.screenshot),
    })
}

#[cfg(all(test, not(any(target_arch = "wasm32", target_os = "android"))))]
mod tests {
    use super::*;

    fn game() -> GameNow {
        GameNow {
            name: "Pokémon Silver Version".into(),
            details: Some("🏅\u{a0}7 · Dex\u{a0}44/66 · 60:41".into()),
            hover: Some("Typhlosion Lv\u{a0}45 · ¥12,345".into()),
            started_at: 1000,
        }
    }

    #[test]
    fn nothing_shows_until_it_is_turned_on() {
        assert_eq!(
            presence(&DiscordSettings::default(), Some(&game()), None),
            None
        );
    }

    #[test]
    fn each_switch_hides_its_own_line() {
        let mut settings = DiscordSettings {
            enabled: true,
            ..Default::default()
        };
        let all = presence(&settings, Some(&game()), None).unwrap();
        assert_eq!(all.details, "Pokémon Silver Version");
        assert_eq!(
            all.state.as_deref(),
            Some("🏅\u{a0}7 · Dex\u{a0}44/66 · 60:41")
        );
        assert_eq!(
            all.hover.as_deref(),
            Some("Typhlosion Lv\u{a0}45 · ¥12,345")
        );
        assert_eq!(all.started_at, Some(1000));
        assert!(!all.screenshot);
        assert_eq!(all.image, None);
        settings.game = false;
        settings.details = false;
        settings.play_time = false;
        let bare = presence(&settings, Some(&game()), None).unwrap();
        assert_eq!(bare.details, "Playing a game");
        assert_eq!(
            (bare.state, bare.hover, bare.started_at),
            (None, None, None)
        );
        assert_eq!(
            presence(&settings, None, None).unwrap().details,
            "Choosing a game"
        );
    }

    fn width(text: &str) -> usize {
        text.chars()
            .map(|c| if u32::from(c) >= 0x1F000 { 2 } else { 1 })
            .sum()
    }

    #[test]
    fn the_longest_pokemon_line_fits_the_profile_popout() {
        let longest = "🏅\u{a0}16 · Dex\u{a0}251/251 · 999:59";
        assert_eq!(width(longest), 28);
        assert!(width(longest) <= 30);
        assert!(width(&game().details.unwrap()) <= 30);
    }

    #[test]
    fn the_picture_shows_only_with_its_switch() {
        let mut settings = DiscordSettings {
            enabled: true,
            ..Default::default()
        };
        let url = Some("https://gb.lemon.industries/f/ab?v=1".to_string());
        let off = presence(&settings, Some(&game()), url.clone()).unwrap();
        assert_eq!(off.image, None);
        settings.screenshot = true;
        let on = presence(&settings, Some(&game()), url.clone()).unwrap();
        assert_eq!(on.image, url);
        assert_eq!(presence(&settings, None, url).unwrap().image, None);
    }

    #[test]
    fn a_shot_waits_for_the_interval_a_running_game_and_a_new_frame() {
        let start = std::time::Instant::now();
        let every = ShotInterval::Minute;
        let later = |secs| start + std::time::Duration::from_secs(secs);
        let paused = Some(("red", false));
        let on = Some(("red", true));
        let mut shots = Shots::default();
        assert_eq!(shots.due(start, every, paused, || 1), Shot::Forget);
        assert_eq!(shots.due(start, every, paused, || 1), Shot::Wait);
        assert_eq!(shots.due(start, every, on, || unreachable!()), Shot::Wait);
        assert_eq!(
            shots.due(later(9), every, on, || unreachable!()),
            Shot::Wait
        );
        assert_eq!(shots.due(later(10), every, on, || 1), Shot::Take);
        assert_eq!(shots.due(later(40), every, on, || 2), Shot::Wait);
        assert_eq!(shots.due(later(70), every, on, || 1), Shot::Wait);
        assert_eq!(shots.due(later(100), every, on, || 2), Shot::Wait);
        assert_eq!(shots.due(later(130), every, on, || 2), Shot::Take);
        assert_eq!(shots.due(later(500), every, paused, || 3), Shot::Wait);
        assert_eq!(
            shots.due(later(500), every, on, || unreachable!()),
            Shot::Wait
        );
        assert_eq!(shots.due(later(510), every, on, || 3), Shot::Take);
    }

    #[test]
    fn a_pause_keeps_the_picture() {
        let start = std::time::Instant::now();
        let every = ShotInterval::Minute;
        let mut shots = Shots::default();
        shots.due(start, every, Some(("red", true)), || 1);
        for at in [0, 10, 300, 3000] {
            let now = start + std::time::Duration::from_secs(at);
            assert_eq!(
                shots.due(now, every, Some(("red", false)), || unreachable!()),
                Shot::Wait
            );
        }
    }

    #[test]
    fn another_game_forgets_the_picture_and_shoots_after_ten_seconds() {
        let start = std::time::Instant::now();
        let every = ShotInterval::FiveMinutes;
        let later = |secs| start + std::time::Duration::from_secs(secs);
        let mut shots = Shots::default();
        shots.due(start, every, Some(("red", true)), || 1);
        shots.due(start, every, Some(("red", true)), || 1);
        assert_eq!(
            shots.due(later(10), every, Some(("red", true)), || 1),
            Shot::Take
        );
        let blue = Some(("blue", true));
        assert_eq!(shots.due(later(20), every, blue, || 1), Shot::Forget);
        assert_eq!(shots.due(later(21), every, blue, || 1), Shot::Wait);
        assert_eq!(shots.due(later(31), every, blue, || 1), Shot::Take);
    }

    #[test]
    fn a_session_starting_or_ending_forgets_the_picture() {
        let start = std::time::Instant::now();
        let every = ShotInterval::Minute;
        let mut shots = Shots::default();
        shots.due(start, every, Some(("red", true)), || 1);
        assert_eq!(
            shots.due(start, every, Some(("together red", true)), || 1),
            Shot::Forget
        );
        assert_eq!(shots.due(start, every, None, || 1), Shot::Forget);
        assert_eq!(shots.due(start, every, None, || unreachable!()), Shot::Wait);
        assert_eq!(
            shots.due(start, every, Some(("red", true)), || 1),
            Shot::Forget
        );
    }

    #[test]
    fn the_picture_url_follows_the_relay_address() {
        use crate::online::ServerAddress;
        let secure = ServerAddress::parse("wss://gb.lemon.industries/relay").unwrap();
        assert_eq!(
            screen_url(&secure, "/f/ab12", 3),
            "https://gb.lemon.industries/f/ab12?v=3"
        );
        let port = ServerAddress::parse("gb.lemon.industries:8443").unwrap();
        assert_eq!(
            screen_url(&port, "/f/ab12", 1),
            "https://gb.lemon.industries:8443/f/ab12?v=1"
        );
        let plain = ServerAddress::parse("ws://192.168.1.5:13707").unwrap();
        assert_eq!(
            screen_url(&plain, "/f/ab12", 7),
            "http://192.168.1.5:13707/f/ab12?v=7"
        );
    }
}

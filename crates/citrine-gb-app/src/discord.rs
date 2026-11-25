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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, EnumIter)]
pub enum ShotScreens {
    #[default]
    Both,
    Top,
    Bottom,
}

impl ShotScreens {
    pub fn only(self) -> Option<usize> {
        match self {
            Self::Both => None,
            Self::Top => Some(0),
            Self::Bottom => Some(1),
        }
    }
}

impl std::fmt::Display for ShotScreens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Both => "Both",
            Self::Top => "Top",
            Self::Bottom => "Bottom",
        })
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
    pub screens: ShotScreens,
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
            screens: ShotScreens::default(),
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
    pub fn may_take(&self, now: std::time::Instant, every: ShotInterval) -> bool {
        self.running_since
            .is_none_or(|since| now.duration_since(since).as_secs() >= FIRST_SHOT_AFTER)
            && self
                .checked
                .is_none_or(|at| now.duration_since(at).as_secs() >= every.seconds())
    }

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
pub const SHOT_SIDE: usize = 256;

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub fn screen_png(shoot: impl Fn(usize) -> (usize, usize, Vec<u8>)) -> Result<Vec<u8>, usize> {
    let native = shoot(1);
    if native.0 == 0 || native.1 == 0 {
        return Err(0);
    }
    let sharp = SHOT_SIDE.div_ceil(native.0.max(native.1).max(1)).max(1);
    let mut smallest = usize::MAX;
    for scale in (1..=sharp).rev() {
        let (width, height, rgba) = if scale == 1 {
            native.clone()
        } else {
            shoot(scale)
        };
        let picture = Picture::framed(width, height, &rgba);
        for exact in [true, false] {
            let png = picture.png(exact).unwrap_or_default();
            if citrine_gb_net::check_screen(&png).is_ok() {
                return Ok(png);
            }
            smallest = smallest.min(png.len());
        }
    }
    let (width, height, rgba) = halved(native);
    let png = Picture::framed(width, height, &rgba)
        .png(false)
        .unwrap_or_default();
    match citrine_gb_net::check_screen(&png) {
        Ok(()) => Ok(png),
        Err(_) => Err(smallest.min(png.len())),
    }
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
struct Picture {
    side: [usize; 2],
    at: [usize; 2],
    size: [usize; 2],
    rgba: Vec<u8>,
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
impl Picture {
    fn framed(width: usize, height: usize, rgba: &[u8]) -> Self {
        let (long, short) = (width.max(height), width.min(height));
        let side = if long * 4 > short * 5 {
            [long, long]
        } else {
            [width, height]
        };
        Self {
            side,
            at: [(side[0] - width) / 2, (side[1] - height) / 2],
            size: [width, height],
            rgba: rgba.to_vec(),
        }
    }

    fn png(&self, exact: bool) -> Option<Vec<u8>> {
        let [width, height] = self.size;
        if width == 0 || height == 0 || self.rgba.len() != width * height * 4 {
            return None;
        }
        match self.palette(exact) {
            Some((palette, indices)) => self.indexed(&palette, &indices),
            None => self.full(),
        }
    }

    fn palette(&self, exact: bool) -> Option<(Vec<[u8; 3]>, Vec<u8>)> {
        let pixels = self.rgba.as_chunks::<4>().0;
        if !exact {
            let opaque: Vec<u8> = pixels
                .iter()
                .flat_map(|&[r, g, b, _]| [r, g, b, 255])
                .collect();
            let quant = color_quant::NeuQuant::new(10, 255, &opaque);
            let palette = quant.color_map_rgb().as_chunks::<3>().0.to_vec();
            let mut found = std::collections::HashMap::new();
            let indices = opaque
                .as_chunks::<4>()
                .0
                .iter()
                .map(|pixel| {
                    *found
                        .entry(*pixel)
                        .or_insert_with(|| quant.index_of(pixel) as u8 + 1)
                })
                .collect();
            return Some((palette, indices));
        }
        let mut palette = Vec::new();
        let mut found = std::collections::HashMap::new();
        let mut indices = Vec::with_capacity(pixels.len());
        for &[r, g, b, _] in pixels {
            let next = palette.len();
            let index = *found.entry([r, g, b]).or_insert(next);
            if index == next {
                if next == 255 {
                    return None;
                }
                palette.push([r, g, b]);
            }
            indices.push(index as u8 + 1);
        }
        Some((palette, indices))
    }

    fn indexed(&self, palette: &[[u8; 3]], indices: &[u8]) -> Option<Vec<u8>> {
        let mut plte = vec![0; 3];
        plte.extend(palette.iter().flatten());
        let rows = self.place(0, indices.chunks(self.size[0]));
        self.encode(png::ColorType::Indexed, &rows, |encoder| {
            encoder.set_palette(plte);
            encoder.set_trns(vec![0]);
        })
    }

    fn full(&self) -> Option<Vec<u8>> {
        let opaque: Vec<u8> = self
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[r, g, b, _]| [r, g, b, 255])
            .collect();
        let rows = self.place([0; 4], opaque.as_chunks::<4>().0.chunks(self.size[0]));
        let bytes: Vec<u8> = rows.into_iter().flatten().collect();
        self.encode(png::ColorType::Rgba, &bytes, |_| {})
    }

    fn place<T: Copy>(&self, empty: T, rows: std::slice::Chunks<'_, T>) -> Vec<T> {
        let [side_x, side_y] = self.side;
        let mut out = vec![empty; side_x * side_y];
        for (y, row) in rows.enumerate() {
            let start = (self.at[1] + y) * side_x + self.at[0];
            out[start..start + row.len()].copy_from_slice(row);
        }
        out
    }

    fn encode(
        &self,
        color: png::ColorType,
        data: &[u8],
        setup: impl FnOnce(&mut png::Encoder<'_, &mut Vec<u8>>),
    ) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, self.side[0] as u32, self.side[1] as u32);
            encoder.set_color(color);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_compression(png::Compression::High);
            setup(&mut encoder);
            let mut writer = encoder.write_header().ok()?;
            writer.write_image_data(data).ok()?;
        }
        Some(out)
    }
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn halved((width, height, rgba): (usize, usize, Vec<u8>)) -> (usize, usize, Vec<u8>) {
    let (half_w, half_h) = ((width / 2).max(1), (height / 2).max(1));
    let mut out = Vec::with_capacity(half_w * half_h * 4);
    for y in 0..half_h {
        for x in 0..half_w {
            for channel in 0..4 {
                let sum: usize = [(0, 0), (1, 0), (0, 1), (1, 1)]
                    .iter()
                    .map(|&(dx, dy)| {
                        let (sx, sy) = ((2 * x + dx).min(width - 1), (2 * y + dy).min(height - 1));
                        usize::from(rgba[(sy * width + sx) * 4 + channel])
                    })
                    .sum();
                out.push((sum / 4) as u8);
            }
        }
    }
    (half_w, half_h, out)
}

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

    fn console_shot(rom: &[u8], kind: citrine_gb::console::ConsoleKind, frames: usize) -> Made {
        console_shot_of(rom, kind, frames, ShotScreens::Both)
    }

    fn console_shot_of(
        rom: &[u8],
        kind: citrine_gb::console::ConsoleKind,
        frames: usize,
        screens: ShotScreens,
    ) -> Made {
        use citrine_gb::console::Console;
        let firmware = citrine_gb::machine::Firmware::default();
        let mut machine =
            citrine_gb::machine::Machine::load(&citrine_gb::rom::Rom::new(rom), kind, &firmware)
                .expect("the game loads");
        for _ in 0..frames {
            machine.run_frame();
        }
        let mut emulator = crate::emulator::Emulator::default();
        emulator.put_machine(machine);
        emulator.finish_picture();
        let frame = emulator.published_frame().to_vec();
        let shot = screen_png(|scale| emulator.screenshot_of_screen(&frame, scale, screens.only()));
        Made { frame, shot }
    }

    #[cfg(any(feature = "gba", feature = "nds"))]
    fn unpacked(packed: &[u8]) -> Vec<u8> {
        crate::storage::decompress(packed).expect("the game unpacks")
    }

    struct Made {
        frame: Vec<u8>,
        shot: Result<Vec<u8>, usize>,
    }

    fn colors(rgba: &[u8]) -> usize {
        let mut seen: Vec<[u8; 4]> = rgba.as_chunks::<4>().0.to_vec();
        seen.sort_unstable();
        seen.dedup();
        seen.len()
    }

    fn goes_out(name: &str, made: Made, side: u32) -> crate::media::Frame {
        let png = match made.shot {
            Ok(png) => png,
            Err(bytes) => {
                panic!("{name}: no picture under the relay limit, smallest {bytes} bytes")
            }
        };
        assert_eq!(citrine_gb_net::check_screen(&png), Ok(()), "{name}");
        let picture = crate::media::decode_png(&png).expect("the picture decodes");
        eprintln!(
            "discord shot {name}: {} bytes, {}x{}",
            png.len(),
            picture.width,
            picture.height
        );
        if let Some(dir) = std::env::var_os("CITRINE_DISCORD_SHOTS") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(std::path::Path::new(&dir).join(format!("{name}.png")), &png).unwrap();
        }
        assert!(colors(&made.frame) > 1, "{name}: the game drew something");
        assert!(
            picture.width.max(picture.height) >= side,
            "{name}: the picture keeps every pixel of the screen"
        );
        picture
    }

    #[test]
    fn a_game_boy_screen_goes_to_discord() {
        use citrine_gb::console::ConsoleKind;
        let rom = include_bytes!("../homebrew/porklike.gb");
        let picture = goes_out("gb", console_shot(rom, ConsoleKind::Gb, 600), 160);
        assert_eq!(
            picture.width * 144,
            picture.height * 160,
            "an old relay takes it"
        );
    }

    #[test]
    fn a_game_boy_color_screen_goes_to_discord() {
        use citrine_gb::console::ConsoleKind;
        let rom = include_bytes!("../homebrew/ucity.gbc");
        goes_out("gbc", console_shot(rom, ConsoleKind::Gbc, 600), 160);
    }

    #[cfg(feature = "gba")]
    #[test]
    fn a_game_boy_advance_screen_goes_to_discord() {
        use citrine_gb::console::ConsoleKind;
        let games: [(&str, &[u8]); 2] = [
            (
                "gba-pentris",
                include_bytes!("../homebrew/gba/pentris.gba.br"),
            ),
            (
                "gba-hyperspace-roll",
                include_bytes!("../homebrew/gba/hyperspace_roll.gba.br"),
            ),
        ];
        for (name, rom) in games {
            goes_out(
                name,
                console_shot(&unpacked(rom), ConsoleKind::Gba, 600),
                240,
            );
        }
    }

    #[cfg(feature = "nds")]
    #[test]
    fn a_ds_screen_with_3d_goes_to_discord() {
        use citrine_gb::console::ConsoleKind;
        let rom = include_bytes!("../fixtures/nds/blocksds-lighting_cube.nds");
        let picture = goes_out("ds-3d-cube", console_shot(rom, ConsoleKind::Nds, 90), 384);
        assert!(
            colors(&picture.rgba) > 32,
            "the lit cube's shading reaches the picture"
        );
        let pong = include_bytes!("../homebrew/nds/pong_nds.nds.br");
        goes_out(
            "ds-pong",
            console_shot(&unpacked(pong), ConsoleKind::Nds, 300),
            384,
        );
    }

    #[cfg(feature = "nds")]
    #[test]
    fn discord_gets_one_ds_screen_when_asked() {
        use citrine_gb::console::ConsoleKind;
        let rom = include_bytes!("../fixtures/nds/blocksds-lighting_cube.nds");
        let opaque = |picture: &crate::media::Frame| {
            picture
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[3] == 255)
                .count()
        };
        let both = goes_out(
            "ds-both",
            console_shot_of(rom, ConsoleKind::Nds, 90, ShotScreens::Both),
            384,
        );
        assert_eq!(opaque(&both), 256 * 384, "both screens, whole");
        let top = goes_out(
            "ds-top",
            console_shot_of(rom, ConsoleKind::Nds, 90, ShotScreens::Top),
            256,
        );
        assert_eq!(opaque(&top), 256 * 192, "one screen, whole");
        assert!(colors(&top.rgba) > 32, "the top screen holds the lit cube");
        let bottom = goes_out(
            "ds-bottom",
            console_shot_of(rom, ConsoleKind::Nds, 90, ShotScreens::Bottom),
            256,
        );
        assert_eq!(opaque(&bottom), 256 * 192, "one screen, whole");
        assert!(
            colors(&bottom.rgba) <= 32,
            "the bottom screen holds the text"
        );
    }

    #[test]
    fn old_discord_settings_share_both_ds_screens() {
        let old = r#"{"enabled":true,"screenshot":true,"every":"Minute"}"#;
        let settings: DiscordSettings = serde_json::from_str(old).unwrap();
        assert_eq!(settings.screens, ShotScreens::Both);
        assert_eq!(settings.every, ShotInterval::Minute);
    }
}

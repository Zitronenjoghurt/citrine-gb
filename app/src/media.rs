#[cfg(not(target_arch = "wasm32"))]
mod native;
pub mod recording;
#[cfg(not(target_arch = "wasm32"))]
pub mod reveal;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(not(target_arch = "wasm32"))]
pub use native::MediaStore;
#[cfg(not(target_arch = "wasm32"))]
pub use native::recording_temp_path;
#[cfg(target_arch = "wasm32")]
pub use web::MediaStore;

use citrine_gb::gb::GbModel;
use serde::{Deserialize, Serialize};

pub const THUMB_WIDTH: u32 = 160;
pub const THUMB_HEIGHT: u32 = 144;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MediaKind {
    Screenshot,
    Recording,
}

impl MediaKind {
    pub const ALL: [MediaKind; 2] = [MediaKind::Screenshot, MediaKind::Recording];

    pub fn folder(self) -> &'static str {
        match self {
            MediaKind::Screenshot => "screenshots",
            MediaKind::Recording => "recordings",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            MediaKind::Screenshot => "png",
            MediaKind::Recording => "mp4",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MediaKind::Screenshot => "Screenshot",
            MediaKind::Recording => "Recording",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MediaId(String);

impl MediaId {
    fn new(kind: MediaKind, file_name: &str) -> Self {
        Self(format!("{}/{file_name}", kind.folder()))
    }

    pub fn file_name(&self) -> &str {
        self.0.split_once('/').map_or(&self.0, |(_, name)| name)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn kind(&self) -> Option<MediaKind> {
        let (folder, _) = self.0.split_once('/')?;
        MediaKind::ALL
            .into_iter()
            .find(|kind| kind.folder() == folder)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaInfo {
    pub kind: MediaKind,
    pub game: String,
    pub rom_key: Option<String>,
    pub model: Option<GbModel>,
    pub created_at: u64,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
    pub length_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Frame {
    fn is_valid(&self) -> bool {
        self.width > 0
            && self.height > 0
            && self.rgba.len() == self.width as usize * self.height as usize * 4
    }
}

#[derive(Debug, Clone)]
pub struct MediaItem {
    pub id: MediaId,
    pub info: MediaInfo,
    pub thumbnail: Option<Frame>,
}

#[derive(Debug, Clone)]
pub struct NewMedia {
    pub kind: MediaKind,
    pub game: String,
    pub rom_key: Option<String>,
    pub model: GbModel,
    pub created_at: u64,
    pub width: u32,
    pub height: u32,
    pub length_ms: Option<u64>,
    pub frame: Frame,
}

impl NewMedia {
    pub fn current(kind: MediaKind, emulator: &crate::emulator::Emulator) -> Self {
        let gb = &emulator.gb;
        Self {
            kind,
            game: gb.cartridge.header.title.trim().to_string(),
            rom_key: emulator.rom_key().map(str::to_string),
            model: gb.model,
            created_at: crate::storage::now_unix(),
            width: THUMB_WIDTH,
            height: THUMB_HEIGHT,
            length_ms: None,
            frame: Frame {
                width: THUMB_WIDTH,
                height: THUMB_HEIGHT,
                rgba: gb.frame().as_slice().to_vec(),
            },
        }
    }

    fn info(&self, bytes: u64) -> MediaInfo {
        MediaInfo {
            kind: self.kind,
            game: self.game.trim().to_string(),
            rom_key: self.rom_key.clone(),
            model: Some(self.model),
            created_at: self.created_at,
            bytes,
            width: self.width,
            height: self.height,
            length_ms: self.length_ms,
        }
    }
}

pub fn save_screenshot(
    emulator: &crate::emulator::Emulator,
    store: &mut MediaStore,
    scale: usize,
) -> Result<MediaItem, String> {
    if !emulator.gb.cartridge.has_rom_loaded {
        return Err("Load a ROM before taking a screenshot".to_string());
    }
    let (width, height, rgba) = emulator.screenshot(scale);
    let png = crate::utils::screenshot::encode_png(width, height, &rgba)
        .ok_or_else(|| "Could not encode the screenshot".to_string())?;
    let mut new = NewMedia::current(MediaKind::Screenshot, emulator);
    new.width = width as u32;
    new.height = height as u32;
    store
        .add_screenshot(new, &png)
        .map_err(|err| format!("Could not save the screenshot: {err}"))
}

pub fn export(
    store: &MediaStore,
    id: &MediaId,
    tx: std::sync::mpsc::Sender<crate::utils::file_saver::SaveOutcome>,
) {
    let kind = id.kind().unwrap_or(MediaKind::Screenshot);
    let filter = match kind {
        MediaKind::Screenshot => "PNG image",
        MediaKind::Recording => "MP4 video",
    };
    let saver = crate::utils::file_saver::FileSaver::new(id.file_name())
        .add_filter(filter, &[kind.extension()]);
    #[cfg(not(target_arch = "wasm32"))]
    saver.dispatch_copy(store.path(id), tx);
    #[cfg(target_arch = "wasm32")]
    store.read(id, move |blob| match blob {
        Some(blob) => saver.dispatch_blob(&blob, tx),
        None => {
            let _ = tx.send(crate::utils::file_saver::SaveOutcome::Failed(
                "the file is missing".to_string(),
            ));
        }
    });
}

pub fn file_stem(game: &str, created_at: u64) -> String {
    let title: String = game
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => ' ',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let title = title.trim_matches('.');
    let title = if title.is_empty() { "Citrine" } else { title };
    format!("{title} {}", local_stamp(created_at, "%Y-%m-%d %H-%M-%S"))
}

pub fn local_stamp(unix: u64, format: &str) -> String {
    let Ok(timestamp) = jiff::Timestamp::from_second(unix as i64) else {
        return String::new();
    };
    timestamp
        .to_zoned(jiff::tz::TimeZone::system())
        .strftime(format)
        .to_string()
}

fn unique_file_name(stem: &str, extension: &str, taken: impl Fn(&str) -> bool) -> String {
    let first = format!("{stem}.{extension}");
    if !taken(&first) {
        return first;
    }
    (2..)
        .map(|n| format!("{stem} ({n}).{extension}"))
        .find(|name| !taken(name))
        .unwrap_or(first)
}

#[cfg(not(target_arch = "wasm32"))]
fn game_from_file_name(file_name: &str) -> String {
    let stem = file_name
        .rsplit_once('.')
        .map_or(file_name, |(stem, _)| stem);
    let stem = match stem.rsplit_once(" (") {
        Some((head, tail))
            if tail.ends_with(')') && tail[..tail.len() - 1].parse::<u32>().is_ok() =>
        {
            head
        }
        _ => stem,
    };
    let bytes = stem.as_bytes();
    let stamp_len = "0000-00-00 00-00-00".len();
    if bytes.len() > stamp_len {
        let (head, stamp) = stem.split_at(stem.len() - stamp_len);
        let looks_like_stamp = stamp
            .chars()
            .zip("0000-00-00 00-00-00".chars())
            .all(|(c, p)| if p == '0' { c.is_ascii_digit() } else { c == p });
        if looks_like_stamp && head.ends_with(' ') {
            return head.trim().to_string();
        }
    }
    stem.trim().to_string()
}

fn sort_newest_first(items: &mut [MediaItem]) {
    items.sort_by(|a, b| {
        b.info
            .created_at
            .cmp(&a.info.created_at)
            .then_with(|| b.id.cmp(&a.id))
    });
}

pub fn thumbnail_of(frame: &Frame) -> Option<Frame> {
    if !frame.is_valid() {
        return None;
    }
    let scale = (frame.width.div_ceil(THUMB_WIDTH)).max(frame.height.div_ceil(THUMB_HEIGHT));
    let scale = scale.max(1);
    let width = (frame.width / scale).max(1);
    let height = (frame.height / scale).max(1);
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            let mut sum = [0u32; 4];
            for dy in 0..scale {
                for dx in 0..scale {
                    let at = (((y * scale + dy) * frame.width + x * scale + dx) * 4) as usize;
                    for (channel, total) in sum.iter_mut().enumerate() {
                        *total += frame.rgba[at + channel] as u32;
                    }
                }
            }
            let count = scale * scale;
            rgba.extend(sum.iter().map(|total| (total / count) as u8));
        }
    }
    Some(Frame {
        width,
        height,
        rgba,
    })
}

pub fn encode_thumbnail(frame: &Frame) -> Option<Vec<u8>> {
    let thumb = thumbnail_of(frame)?;
    crate::utils::screenshot::encode_png(thumb.width as usize, thumb.height as usize, &thumb.rgba)
}

pub fn decode_png(bytes: &[u8]) -> Option<Frame> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(
        png::Transformations::normalize_to_color8() | png::Transformations::ALPHA,
    );
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::GrayscaleAlpha => buf
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|&[gray, alpha]| [gray, gray, gray, alpha])
            .collect(),
        _ => return None,
    };
    let frame = Frame {
        width: info.width,
        height: info.height,
        rgba,
    };
    frame.is_valid().then_some(frame)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let reader = decoder.read_info().ok()?;
    let info = reader.info();
    Some((info.width, info.height))
}

pub fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let value = bytes as f64;
    if value < KB {
        format!("{bytes} B")
    } else if value < KB * KB {
        format!("{:.0} KB", value / KB)
    } else if value < KB * KB * KB {
        format!("{:.1} MB", value / (KB * KB))
    } else {
        format!("{:.1} GB", value / (KB * KB * KB))
    }
}

pub fn format_length(length_ms: u64) -> String {
    let seconds = length_ms / 1000;
    let (hours, minutes, seconds) = (seconds / 3600, (seconds / 60) % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_keep_the_title_readable_and_drop_unsafe_characters() {
        let stem = file_stem("POKEMON RED/BLUE: \"x\"", 0);
        let (title, stamp) = stem.split_at(stem.len() - 19);
        assert_eq!(title, "POKEMON RED BLUE x ");
        assert_eq!(stamp.len(), 19);
        assert!(!stem.contains('/') && !stem.contains(':'));
    }

    #[test]
    fn an_untitled_game_is_named_after_citrine() {
        assert!(file_stem("   ", 0).starts_with("Citrine "));
        assert!(file_stem("...", 0).starts_with("Citrine "));
    }

    #[test]
    fn a_taken_name_gets_a_number() {
        let taken = ["Tetris 1.png", "Tetris 1 (2).png"];
        let name = unique_file_name("Tetris 1", "png", |n| taken.contains(&n));
        assert_eq!(name, "Tetris 1 (3).png");
        assert_eq!(unique_file_name("Zelda", "mp4", |_| false), "Zelda.mp4");
    }

    #[test]
    fn the_game_comes_back_out_of_a_file_name() {
        assert_eq!(
            game_from_file_name("TETRIS 2026-10-26 01-30-00.png"),
            "TETRIS"
        );
        assert_eq!(
            game_from_file_name("TETRIS 2026-10-26 01-30-00 (2).png"),
            "TETRIS"
        );
        assert_eq!(game_from_file_name("holiday.png"), "holiday");
        assert_eq!(
            game_from_file_name("2026-10-26 01-30-00.png"),
            "2026-10-26 01-30-00"
        );
    }

    #[test]
    fn a_thumbnail_fits_the_game_boy_screen() {
        let frame = Frame {
            width: 640,
            height: 576,
            rgba: vec![200; 640 * 576 * 4],
        };
        let thumb = thumbnail_of(&frame).expect("thumbnail");
        assert_eq!((thumb.width, thumb.height), (160, 144));
        assert!(thumb.rgba.iter().all(|&b| b == 200));
        let small = Frame {
            width: 160,
            height: 144,
            rgba: vec![9; 160 * 144 * 4],
        };
        assert_eq!(thumbnail_of(&small), Some(small));
    }

    #[test]
    fn a_thumbnail_survives_png() {
        let frame = Frame {
            width: 2,
            height: 1,
            rgba: vec![1, 2, 3, 255, 4, 5, 6, 255],
        };
        let png = encode_thumbnail(&frame).expect("encodes");
        assert_eq!(decode_png(&png), Some(frame));
        assert_eq!(png_size(&png), Some((2, 1)));
    }

    #[test]
    fn sizes_and_lengths_read_plainly() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(20 * 1024), "20 KB");
        assert_eq!(format_size(3 * 1024 * 1024 + 200 * 1024), "3.2 MB");
        assert_eq!(format_length(42_500), "0:42");
        assert_eq!(format_length(3_725_000), "1:02:05");
    }
}

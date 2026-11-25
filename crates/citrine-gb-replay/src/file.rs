use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

pub const FORMAT: u32 = 1;

const HEADER: &str = "replay.json";
const START: &str = "start.state";
const INPUTS: &str = "inputs.bin";
const CHECKS: &str = "checks.bin";
const PERF_FRAMES: &str = "perf/frames.csv";
const PERF_INFO: &str = "perf/info.json";
const PERF_GPU: &str = "perf/gpu.txt";
const VIDEO: &str = "video.mp4";
const THUMB: &str = "thumb.png";
const ENTRY_LIMIT: u64 = 256 << 20;
const VIDEO_LIMIT: u64 = 2 << 30;
const LZ4_MAX_RATIO: usize = 255;

#[derive(Debug, thiserror::Error)]
pub enum ReplayError {
    #[error("zip: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("replay.json: {0}")]
    Header(#[from] serde_json::Error),
    #[error("the file has no {0}")]
    Missing(&'static str),
    #[error("{0} is damaged")]
    Damaged(&'static str),
    #[error("the file needs a newer version of Citrine (format {0})")]
    Newer(u32),
    #[error("{0} is too big")]
    TooBig(&'static str),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Header {
    pub format: u32,
    pub console: String,
    pub game: String,
    pub crc32: u32,
    pub sha1: String,
    pub app: String,
    pub start_unix: u64,
    pub frames: u64,
    pub check_from: u64,
    pub frame_rate: f64,
    pub created_unix: u64,
    pub last_hash: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_inputs: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Input {
    Keys(u16),
    Touch(Option<(u8, u8)>),
    Reset,
    Mic(Vec<i16>),
    Motion(f32, f32, f32),
    Sun(f32),
    Camera(Vec<u8>),
    Cheats(Vec<String>),
    HoldClock(f64),
    SetClock(i64),
    ShiftClock(i64, u64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub frame: u64,
    pub input: Input,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Replay {
    pub header: Header,
    pub start: Vec<u8>,
    pub events: Vec<Event>,
    pub checks: Vec<u64>,
    pub perf: Option<Perf>,
    pub video: Option<Vec<u8>>,
    pub thumb: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Perf {
    pub frames: Vec<u8>,
    pub info: Vec<u8>,
    pub gpu: Option<Vec<u8>>,
}

impl Replay {
    pub fn write(&self) -> Result<Vec<u8>, ReplayError> {
        if self
            .video
            .as_ref()
            .is_some_and(|video| video.len() as u64 > VIDEO_LIMIT)
        {
            return Err(ReplayError::TooBig(VIDEO));
        }
        let mut header = self.header.clone();
        header.format = FORMAT;
        let json = serde_json::to_vec_pretty(&header)?;
        let inputs = encode_events(&self.events);
        let checks: Vec<u8> = self.checks.iter().flat_map(|c| c.to_le_bytes()).collect();
        let deflated = options(CompressionMethod::Deflated);
        let stored = options(CompressionMethod::Stored);
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes, how) in [
            (HEADER, &json[..], deflated),
            (START, &self.start[..], stored),
            (INPUTS, &inputs[..], stored),
            (CHECKS, &checks[..], stored),
        ] {
            zip.start_file(name, how)?;
            zip.write_all(bytes)?;
        }
        if let Some(perf) = &self.perf {
            let gpu = perf.gpu.as_deref().map(|gpu| (PERF_GPU, gpu));
            for (name, bytes) in [(PERF_FRAMES, &perf.frames[..]), (PERF_INFO, &perf.info[..])]
                .into_iter()
                .chain(gpu)
            {
                zip.start_file(name, deflated)?;
                zip.write_all(bytes)?;
            }
        }
        let video = self.video.as_deref().map(|video| (VIDEO, video));
        let thumb = self.thumb.as_deref().map(|thumb| (THUMB, thumb));
        for (name, bytes) in video.into_iter().chain(thumb) {
            zip.start_file(name, stored)?;
            zip.write_all(bytes)?;
        }
        Ok(zip.finish()?.into_inner())
    }

    pub fn read(bytes: &[u8]) -> Result<Self, ReplayError> {
        let mut zip = ZipArchive::new(Cursor::new(bytes))?;
        let header: Header = serde_json::from_slice(&need(&mut zip, HEADER)?)?;
        if header.format > FORMAT {
            return Err(ReplayError::Newer(header.format));
        }
        let start = need(&mut zip, START)?;
        let events = decode_events(&need(&mut zip, INPUTS)?)?;
        let checks = need(&mut zip, CHECKS)?;
        if !checks.len().is_multiple_of(8) {
            return Err(ReplayError::Damaged(CHECKS));
        }
        let checks = checks
            .as_chunks::<8>()
            .0
            .iter()
            .map(|c| u64::from_le_bytes(*c))
            .collect();
        let perf = match need(&mut zip, PERF_FRAMES) {
            Ok(frames) => Some(Perf {
                frames,
                info: need(&mut zip, PERF_INFO)?,
                gpu: maybe(&mut zip, PERF_GPU)?,
            }),
            Err(ReplayError::Missing(_)) => None,
            Err(err) => return Err(err),
        };
        Ok(Self {
            header,
            start,
            events,
            checks,
            perf,
            video: maybe_up_to(&mut zip, VIDEO, VIDEO_LIMIT)?,
            thumb: maybe(&mut zip, THUMB)?,
        })
    }

    pub fn video(bytes: &[u8]) -> Result<Option<Vec<u8>>, ReplayError> {
        let mut zip = ZipArchive::new(Cursor::new(bytes))?;
        let header: Header = serde_json::from_slice(&need(&mut zip, HEADER)?)?;
        if header.format > FORMAT {
            return Err(ReplayError::Newer(header.format));
        }
        maybe_up_to(&mut zip, VIDEO, VIDEO_LIMIT)
    }
}

fn options(method: CompressionMethod) -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(method)
        .last_modified_time(zip::DateTime::default())
}

fn need(zip: &mut ZipArchive<Cursor<&[u8]>>, name: &'static str) -> Result<Vec<u8>, ReplayError> {
    need_up_to(zip, name, ENTRY_LIMIT)
}

fn need_up_to(
    zip: &mut ZipArchive<Cursor<&[u8]>>,
    name: &'static str,
    limit: u64,
) -> Result<Vec<u8>, ReplayError> {
    let file = match zip.by_name(name) {
        Ok(file) => file,
        Err(zip::result::ZipError::FileNotFound) => return Err(ReplayError::Missing(name)),
        Err(err) => return Err(err.into()),
    };
    if file.size() > limit {
        return Err(ReplayError::Damaged(name));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(ReplayError::Damaged(name));
    }
    Ok(bytes)
}

fn maybe(
    zip: &mut ZipArchive<Cursor<&[u8]>>,
    name: &'static str,
) -> Result<Option<Vec<u8>>, ReplayError> {
    maybe_up_to(zip, name, ENTRY_LIMIT)
}

fn maybe_up_to(
    zip: &mut ZipArchive<Cursor<&[u8]>>,
    name: &'static str,
    limit: u64,
) -> Result<Option<Vec<u8>>, ReplayError> {
    match need_up_to(zip, name, limit) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(ReplayError::Missing(_)) => Ok(None),
        Err(err) => Err(err),
    }
}

fn encode_events(events: &[Event]) -> Vec<u8> {
    let mut last = 0;
    let pairs: Vec<(u64, &Input)> = events
        .iter()
        .map(|e| {
            let gap = e.frame - last;
            last = e.frame;
            (gap, &e.input)
        })
        .collect();
    let raw = postcard::to_stdvec(&pairs).unwrap_or_default();
    lz4_flex::compress_prepend_size(&raw)
}

fn decode_events(bytes: &[u8]) -> Result<Vec<Event>, ReplayError> {
    let (claimed, body) = bytes
        .split_first_chunk::<4>()
        .ok_or(ReplayError::Damaged(INPUTS))?;
    if u32::from_le_bytes(*claimed) as usize > (body.len() + 16) * LZ4_MAX_RATIO {
        return Err(ReplayError::Damaged(INPUTS));
    }
    let raw =
        lz4_flex::decompress_size_prepended(bytes).map_err(|_| ReplayError::Damaged(INPUTS))?;
    let pairs: Vec<(u64, Input)> =
        postcard::from_bytes(&raw).map_err(|_| ReplayError::Damaged(INPUTS))?;
    let mut frame = 0;
    Ok(pairs
        .into_iter()
        .map(|(gap, input)| {
            frame += gap;
            Event { frame, input }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Replay {
        Replay {
            header: Header {
                format: FORMAT,
                console: "gb".into(),
                game: "Tobu Tobu Girl".into(),
                crc32: 0x1234_5678,
                sha1: "a".repeat(40),
                app: "0.13.0".into(),
                start_unix: 1_800_000_000,
                frames: 600,
                check_from: 60,
                frame_rate: 59.7275,
                created_unix: 1_800_000_010,
                last_hash: 0xDEAD_BEEF,
                no_inputs: None,
            },
            start: vec![7; 5000],
            events: vec![
                Event {
                    frame: 0,
                    input: Input::Keys(0x0001),
                },
                Event {
                    frame: 30,
                    input: Input::Keys(0),
                },
                Event {
                    frame: 31,
                    input: Input::Touch(Some((12, 190))),
                },
                Event {
                    frame: 45,
                    input: Input::Touch(None),
                },
                Event {
                    frame: 400,
                    input: Input::Reset,
                },
            ],
            checks: (0..10).map(|i| i * 0x0101_0101).collect(),
            perf: None,
            video: None,
            thumb: None,
        }
    }

    fn video_only() -> Replay {
        Replay {
            header: Header {
                no_inputs: Some("link cable".into()),
                ..sample().header
            },
            start: Vec::new(),
            events: Vec::new(),
            checks: Vec::new(),
            perf: None,
            video: Some(b"\0\0\0\x18ftypisom".repeat(40)),
            thumb: Some(b"\x89PNG thumb".to_vec()),
        }
    }

    #[test]
    fn a_video_recording_comes_back_with_its_reason() {
        let replay = video_only();
        let bytes = replay.write().unwrap();
        let read = Replay::read(&bytes).unwrap();
        assert_eq!(read, replay);
        assert_eq!(read.header.no_inputs.as_deref(), Some("link cable"));
        assert_eq!(Replay::video(&bytes).unwrap(), replay.video);
        let zip = ZipArchive::new(Cursor::new(&bytes[..])).unwrap();
        assert!(zip.index_for_name(VIDEO).is_some() && zip.index_for_name(THUMB).is_some());
    }

    #[test]
    fn a_replay_without_a_video_has_none() {
        let bytes = sample().write().unwrap();
        assert_eq!(Replay::video(&bytes).unwrap(), None);
        let json = String::from_utf8(
            ZipArchive::new(Cursor::new(&bytes[..]))
                .unwrap()
                .by_name(HEADER)
                .map(|mut file| {
                    let mut text = Vec::new();
                    file.read_to_end(&mut text).unwrap();
                    text
                })
                .unwrap(),
        )
        .unwrap();
        assert!(!json.contains("no_inputs"), "{json}");
    }

    fn with_perf(gpu: bool) -> Replay {
        let mut replay = sample();
        replay.perf = Some(Perf {
            frames: b"tape_frame,update_ms\n1,0.4\n2,0.5\n".to_vec(),
            info: br#"{"refresh_hz":120}"#.to_vec(),
            gpu: gpu.then(|| b"Apple M2".to_vec()),
        });
        replay
    }

    fn fnv1a(bytes: &[u8]) -> u64 {
        bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
        })
    }

    #[test]
    fn a_replay_comes_back_from_its_file_unchanged() {
        let replay = sample();
        let bytes = replay.write().unwrap();
        assert_eq!(Replay::read(&bytes).unwrap(), replay);
    }

    fn with_entry(bytes: &[u8], name: &str, swap: &[u8]) -> Vec<u8> {
        let mut old = ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..old.len() {
            let mut file = old.by_index(i).unwrap();
            let entry = file.name().unwrap().to_string();
            let mut body = Vec::new();
            file.read_to_end(&mut body).unwrap();
            zip.start_file(entry.as_str(), options(CompressionMethod::Stored))
                .unwrap();
            zip.write_all(if entry == name { swap } else { &body[..] })
                .unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn a_replay_from_a_newer_version_is_refused() {
        let bytes = sample().write().unwrap();
        let mut header = sample().header;
        header.format = FORMAT + 1;
        let json = serde_json::to_vec(&header).unwrap();
        let newer = with_entry(&bytes, HEADER, &json);
        assert!(matches!(Replay::read(&newer), Err(ReplayError::Newer(f)) if f == FORMAT + 1));
    }

    #[test]
    fn a_cut_off_file_is_refused() {
        let bytes = sample().write().unwrap();
        for len in 0..bytes.len() {
            assert!(Replay::read(&bytes[..len]).is_err(), "cut at {len}");
        }
    }

    #[test]
    fn inputs_claiming_a_huge_size_are_refused_before_they_allocate() {
        let bytes = sample().write().unwrap();
        let bomb = with_entry(&bytes, INPUTS, &[0xFF, 0xFF, 0xFF, 0x7F, 1, 2, 3]);
        assert!(matches!(
            Replay::read(&bomb),
            Err(ReplayError::Damaged(INPUTS))
        ));
        let short = with_entry(&bytes, INPUTS, &[1, 2]);
        assert!(matches!(
            Replay::read(&short),
            Err(ReplayError::Damaged(INPUTS))
        ));
    }

    #[test]
    fn checks_with_a_partial_hash_are_refused() {
        let bytes = sample().write().unwrap();
        let partial = with_entry(&bytes, CHECKS, &[0; 12]);
        assert!(matches!(
            Replay::read(&partial),
            Err(ReplayError::Damaged(CHECKS))
        ));
    }

    #[test]
    fn frame_times_come_back_with_their_replay() {
        for gpu in [false, true] {
            let replay = with_perf(gpu);
            let bytes = replay.write().unwrap();
            assert_eq!(Replay::read(&bytes).unwrap(), replay);
        }
    }

    #[test]
    fn a_replay_without_frame_times_has_no_perf_entries() {
        let bytes = sample().write().unwrap();
        let zip = ZipArchive::new(Cursor::new(&bytes[..])).unwrap();
        let names: Vec<String> = zip.file_names().map(|n| n.unwrap().into_owned()).collect();
        assert!(names.iter().all(|n| !n.starts_with("perf/")), "{names:?}");
        assert_eq!(names.len(), 4);
    }

    #[test]
    fn a_replay_without_frame_times_keeps_its_bytes() {
        let bytes = sample().write().unwrap();
        assert_eq!((bytes.len(), fnv1a(&bytes)), (5686, 8372990578297755116));
    }

    #[test]
    fn frame_times_without_their_info_are_refused() {
        let bytes = with_perf(false).write().unwrap();
        let mut old = ZipArchive::new(Cursor::new(&bytes[..])).unwrap();
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..old.len() {
            let mut file = old.by_index(i).unwrap();
            let entry = file.name().unwrap().to_string();
            if entry == PERF_INFO {
                continue;
            }
            let mut body = Vec::new();
            file.read_to_end(&mut body).unwrap();
            zip.start_file(entry.as_str(), options(CompressionMethod::Stored))
                .unwrap();
            zip.write_all(&body).unwrap();
        }
        let cut = zip.finish().unwrap().into_inner();
        assert!(matches!(
            Replay::read(&cut),
            Err(ReplayError::Missing(PERF_INFO))
        ));
    }

    #[test]
    fn a_damaged_byte_anywhere_never_panics() {
        let bytes = sample().write().unwrap();
        for at in 0..bytes.len() {
            for flip in [0x01, 0x80, 0xFF] {
                let mut damaged = bytes.clone();
                damaged[at] ^= flip;
                let _ = Replay::read(&damaged);
            }
        }
    }
}

pub mod mp4;
mod thumbnail;

#[cfg(not(target_arch = "wasm32"))]
mod audio;
#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(feature = "openh264-source", allow(dead_code))]
pub mod codec;
#[cfg(not(target_arch = "wasm32"))]
pub mod h264;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod headless;
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(not(target_arch = "wasm32"))]
use native::{Output, Pending, Worker};
#[cfg(target_arch = "wasm32")]
use web::{Output, Pending, Worker};

use crate::media::NewMedia;
use std::time::Duration;
use thumbnail::Thumbnails;

pub const SOURCE_WIDTH: usize = 160;
pub const SOURCE_HEIGHT: usize = 144;
pub const DEFAULT_SCALE: usize = 4;
pub const MAX_SCALE: usize = 8;
pub const VIDEO_TIMESCALE: u32 = 1_048_576;
pub const FRAME_TICKS: u32 = 17_556;
pub const AUDIO_BITRATE: u32 = 256_000;
pub const CHANNELS: usize = 2;

pub fn fps() -> f64 {
    VIDEO_TIMESCALE as f64 / FRAME_TICKS as f64
}

pub fn even_scale(scale: usize) -> usize {
    scale.clamp(2, MAX_SCALE) & !1
}

pub fn video_bitrate(at_default_scale: u32, scale: usize) -> u32 {
    let area = (scale * scale) as u64;
    let default_area = (DEFAULT_SCALE * DEFAULT_SCALE) as u64;
    (at_default_scale as u64 * area / default_area) as u32
}

pub type RecordingInfo = NewMedia;

#[cfg(not(target_arch = "wasm32"))]
const MAX_LENGTH: Option<Duration> = None;
#[cfg(target_arch = "wasm32")]
const MAX_LENGTH: Option<Duration> = web::MAX_LENGTH;

pub fn max_length() -> Option<Duration> {
    MAX_LENGTH
}

#[allow(dead_code)]
pub enum Availability {
    Ready,
    Checking,
    Download,
    Missing(String),
}

pub const CODEC_NOTICE: &str = "OpenH264 Video Codec provided by Cisco Systems, Inc.";

#[cfg(all(not(target_arch = "wasm32"), feature = "openh264-source"))]
pub fn availability(_sample_rate: u32, _scale: usize) -> Availability {
    Availability::Ready
}

#[cfg(all(not(target_arch = "wasm32"), not(feature = "openh264-source")))]
pub fn availability(_sample_rate: u32, _scale: usize) -> Availability {
    match codec::shared().status() {
        codec::Status::Ready => Availability::Ready,
        codec::Status::Downloading => Availability::Checking,
        codec::Status::Missing => Availability::Download,
        codec::Status::Unsupported => {
            Availability::Missing("Recording isn't available on this system".into())
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), not(feature = "openh264-source")))]
pub fn download_codec(download: bool) -> bool {
    codec::shared().answer(download, codec::from_cisco()) == codec::Answer::Downloading
}

#[cfg(any(target_arch = "wasm32", feature = "openh264-source"))]
pub fn download_codec(_download: bool) -> bool {
    false
}

#[cfg(all(not(target_arch = "wasm32"), not(feature = "openh264-source")))]
pub fn take_codec_failure() -> Option<String> {
    codec::shared().take_failure()
}

#[cfg(any(target_arch = "wasm32", feature = "openh264-source"))]
pub fn take_codec_failure() -> Option<String> {
    None
}

#[cfg(all(not(target_arch = "wasm32"), any(test, feature = "openh264-source")))]
fn video_api() -> anyhow::Result<openh264::OpenH264API> {
    Ok(openh264::OpenH264API::from_source())
}

#[cfg(all(
    not(target_arch = "wasm32"),
    not(any(test, feature = "openh264-source"))
))]
fn video_api() -> anyhow::Result<openh264::OpenH264API> {
    codec::shared().load()
}

#[cfg(target_arch = "wasm32")]
pub fn availability(sample_rate: u32, scale: usize) -> Availability {
    web::availability(sample_rate, even_scale(scale))
}

#[cfg(any(test, target_arch = "wasm32"))]
pub fn upscale_rgba(rgba: &[u8], width: usize, height: usize, scale: usize, out: &mut [u8]) {
    let row_len = width * scale * 4;
    for y in 0..height {
        let start = y * scale * row_len;
        let row = &mut out[start..start + row_len];
        for x in 0..width {
            let pixel = &rgba[(y * width + x) * 4..(y * width + x) * 4 + 4];
            for dx in 0..scale {
                row[(x * scale + dx) * 4..(x * scale + dx) * 4 + 4].copy_from_slice(pixel);
            }
        }
        for dy in 1..scale {
            out.copy_within(start..start + row_len, start + dy * row_len);
        }
    }
}

pub struct FinishedRecording {
    pub output: Output,
    pub info: RecordingInfo,
    pub width: usize,
    pub height: usize,
    pub length: Duration,
    pub frames: u64,
    pub thumbnail: Vec<u8>,
}

struct Session {
    worker: Worker,
    info: RecordingInfo,
    scale: usize,
    sample_rate: u32,
    audio_frames: u64,
    video_frames: u64,
    thumbnails: Thumbnails,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum StopReason {
    Asked,
    SoundChanged,
    TooLong,
    TooSlow,
    Failed,
}

pub enum Outcome {
    Saved(Box<FinishedRecording>),
    Empty,
    Failed(anyhow::Error),
}

#[cfg(not(target_arch = "wasm32"))]
fn discard(output: Output) {
    let _ = std::fs::remove_file(output.path);
}

#[cfg(target_arch = "wasm32")]
fn discard(_output: Output) {}

struct Finishing {
    pending: Pending,
    info: RecordingInfo,
    scale: usize,
    frames: u64,
    thumbnail: Vec<u8>,
}

#[derive(Default)]
pub struct ScreenRecorder {
    session: Option<Session>,
    finishing: Vec<Finishing>,
    stopped: Option<StopReason>,
}

impl ScreenRecorder {
    pub fn is_recording(&self) -> bool {
        self.session.is_some()
    }

    pub fn is_saving(&self) -> bool {
        !self.finishing.is_empty()
    }

    pub fn elapsed(&self) -> Duration {
        self.session
            .as_ref()
            .map_or(Duration::ZERO, |s| frames_to_duration(s.video_frames))
    }

    pub fn start(
        &mut self,
        info: RecordingInfo,
        sample_rate: u32,
        scale: usize,
    ) -> anyhow::Result<()> {
        let scale = even_scale(scale);
        #[cfg(not(target_arch = "wasm32"))]
        let worker = Worker::start(crate::media::recording_temp_path()?, sample_rate, scale)?;
        #[cfg(target_arch = "wasm32")]
        let worker = Worker::start(sample_rate, scale)?;
        self.begin(worker, info, sample_rate, scale);
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn start_into(
        &mut self,
        path: std::path::PathBuf,
        info: RecordingInfo,
        sample_rate: u32,
        scale: usize,
    ) -> anyhow::Result<()> {
        let scale = even_scale(scale);
        let worker = Worker::start(path, sample_rate, scale)?;
        self.begin(worker, info, sample_rate, scale);
        Ok(())
    }

    #[cfg(test)]
    pub fn start_with(
        &mut self,
        api: openh264::OpenH264API,
        path: std::path::PathBuf,
        info: RecordingInfo,
        sample_rate: u32,
    ) -> anyhow::Result<()> {
        let worker = Worker::start_with(api, path, sample_rate, DEFAULT_SCALE)?;
        self.begin(worker, info, sample_rate, DEFAULT_SCALE);
        Ok(())
    }

    fn begin(&mut self, worker: Worker, info: RecordingInfo, sample_rate: u32, scale: usize) {
        self.finish(StopReason::Asked);
        self.stopped = None;
        self.session = Some(Session {
            worker,
            info,
            scale,
            sample_rate,
            audio_frames: 0,
            video_frames: 0,
            thumbnails: Thumbnails::default(),
        });
    }

    pub fn capture(&mut self, rgba: &[u8], audio: &[f32], sample_rate: u32) {
        let Some(session) = &mut self.session else {
            return;
        };
        if sample_rate != session.sample_rate {
            self.finish(StopReason::SoundChanged);
            return;
        }
        if !audio.is_empty() {
            session.audio_frames += (audio.len() / CHANNELS) as u64;
            session.worker.audio(audio);
        }
        let target = frames_for_audio(session.audio_frames, session.sample_rate);
        let repeat = target.saturating_sub(session.video_frames);
        if repeat > 0 {
            session.worker.frame(rgba, repeat as u32);
            for _ in 0..repeat {
                session.thumbnails.push(session.video_frames, rgba);
                session.video_frames += 1;
            }
        }
        if let Some(reason) = session.worker.failed_or_full() {
            self.finish(reason);
        } else if MAX_LENGTH.is_some_and(|max| frames_to_duration(session.video_frames) >= max) {
            self.finish(StopReason::TooLong);
        }
    }

    pub fn stop(&mut self) {
        self.finish(StopReason::Asked);
    }

    fn finish(&mut self, reason: StopReason) {
        let Some(session) = self.session.take() else {
            return;
        };
        self.stopped = Some(reason);
        let thumbnail = session.thumbnails.pick(session.video_frames);
        self.finishing.push(Finishing {
            pending: session.worker.finish(),
            info: session.info,
            scale: session.scale,
            frames: session.video_frames,
            thumbnail,
        });
    }

    pub fn take_stopped(&mut self) -> Option<StopReason> {
        self.stopped.take()
    }

    pub fn poll(&mut self) -> Option<Outcome> {
        let index = self
            .finishing
            .iter_mut()
            .position(|f| f.pending.is_done())?;
        let done = self.finishing.remove(index);
        let output = match done.pending.take() {
            Ok(output) => output,
            Err(err) => return Some(Outcome::Failed(err)),
        };
        if done.frames == 0 {
            discard(output);
            return Some(Outcome::Empty);
        }
        Some(Outcome::Saved(Box::new(FinishedRecording {
            output,
            info: done.info,
            width: SOURCE_WIDTH * done.scale,
            height: SOURCE_HEIGHT * done.scale,
            length: frames_to_duration(done.frames),
            frames: done.frames,
            thumbnail: done.thumbnail,
        })))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn wait(&mut self) -> Vec<Outcome> {
        for finishing in &mut self.finishing {
            finishing.pending.wait();
        }
        std::iter::from_fn(|| self.poll()).collect()
    }
}

fn frames_for_audio(audio_frames: u64, sample_rate: u32) -> u64 {
    let ticks = audio_frames as u128 * VIDEO_TIMESCALE as u128;
    let per_frame = FRAME_TICKS as u128 * sample_rate.max(1) as u128;
    ((ticks + per_frame / 2) / per_frame) as u64
}

pub fn frames_to_duration(frames: u64) -> Duration {
    let nanos = frames as u128 * FRAME_TICKS as u128 * 1_000_000_000 / VIDEO_TIMESCALE as u128;
    Duration::from_nanos(nanos as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_frame_of_sound_asks_for_one_frame_of_video() {
        let rate = 48_000;
        let per_frame = rate as f64 / fps();
        let mut audio = 0u64;
        let mut video = 0u64;
        for n in 1..=10_000u64 {
            let next = (n as f64 * per_frame).floor() as u64;
            audio += next - (((n - 1) as f64) * per_frame).floor() as u64;
            let target = frames_for_audio(audio, rate);
            assert_eq!(target - video, 1, "frame {n}");
            video = target;
        }
    }

    #[test]
    fn the_length_follows_the_frame_count() {
        let length = frames_to_duration(59_727);
        assert!((length.as_secs_f64() - 1000.0).abs() < 0.02, "{length:?}");
    }

    #[test]
    fn upscaling_keeps_every_pixel_sharp() {
        let rgba = [1, 2, 3, 4, 5, 6, 7, 8];
        let mut out = [0; 2 * 2 * 2 * 2 * 4];
        upscale_rgba(&rgba, 2, 1, 2, &mut out[..2 * 2 * 2 * 4]);
        assert_eq!(&out[..8], &[1, 2, 3, 4, 1, 2, 3, 4]);
        assert_eq!(&out[8..16], &[5, 6, 7, 8, 5, 6, 7, 8]);
        assert_eq!(&out[16..32], &out[..16]);
    }
}

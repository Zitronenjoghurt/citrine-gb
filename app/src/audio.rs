pub mod pacer;
pub mod queue;
pub mod resampler;
#[cfg(target_arch = "wasm32")]
pub mod web;

use pacer::AudioFeed;
use queue::{QueueConfig, QueueStats, latency_frames};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

#[cfg(not(target_arch = "wasm32"))]
use anyhow::Context;
#[cfg(not(target_arch = "wasm32"))]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub const MIN_LATENCY_MS: u32 = 40;
pub const MAX_LATENCY_MS: u32 = 250;

#[cfg(target_arch = "wasm32")]
pub const DEFAULT_LATENCY_MS: u32 = 100;
#[cfg(not(target_arch = "wasm32"))]
pub const DEFAULT_LATENCY_MS: u32 = 80;

#[cfg(not(target_arch = "wasm32"))]
const DEFAULT_UNKNOWN_CALLBACK_FRAMES: usize = 1024;
#[cfg(not(target_arch = "wasm32"))]
const CALLBACK_SECS: f64 = 0.01;

#[cfg(not(target_arch = "wasm32"))]
type Output = cpal::Stream;
#[cfg(target_arch = "wasm32")]
type Output = web::WebOutput;

#[derive(Default)]
struct Shared {
    volume: AtomicFloat,
    muted: AtomicBool,
}

impl Shared {
    fn gain(&self) -> f32 {
        if self.muted.load(Ordering::Relaxed) {
            0.0
        } else {
            let volume = self.volume.load();
            volume * volume
        }
    }
}

pub struct AudioLink {
    pub sample_rate: u32,
    pub feed: AudioFeed,
}

pub struct Audio {
    stream: Option<Output>,
    shared: Arc<Shared>,
    stats: Arc<QueueStats>,
    sample_rate: u32,
    channel_count: usize,
    supported_buffer_size: (u32, u32),
    device_description: Option<cpal::DeviceDescription>,
    device_name: Option<String>,
    requested_device: Option<String>,
    latency_ms: u32,
    target_frames: usize,
    callback_frames: usize,
    resumed: bool,
    user_activated: bool,
}

impl Default for Audio {
    fn default() -> Self {
        Self::new()
    }
}

impl Audio {
    pub fn new() -> Self {
        let shared = Shared::default();
        shared.volume.store(1.0);

        Self {
            stream: None,
            shared: Arc::new(shared),
            stats: Arc::default(),
            sample_rate: 44100,
            channel_count: 0,
            supported_buffer_size: (0, 0),
            device_description: None,
            device_name: None,
            requested_device: None,
            latency_ms: DEFAULT_LATENCY_MS,
            target_frames: 0,
            callback_frames: 0,
            resumed: false,
            user_activated: false,
        }
    }

    pub fn mark_user_activated(&mut self) {
        self.user_activated = true;
    }

    pub fn is_running(&self) -> bool {
        self.stream.is_some()
    }

    pub fn start(
        &mut self,
        device_name: Option<&str>,
        latency_ms: u32,
    ) -> anyhow::Result<AudioLink> {
        self.stream = None;
        let latency_ms = latency_ms.clamp(MIN_LATENCY_MS, MAX_LATENCY_MS);
        let play_now = !cfg!(target_arch = "wasm32") || self.user_activated;
        let link = self.open(device_name, latency_ms, play_now)?;
        self.requested_device = device_name.map(str::to_owned);
        self.latency_ms = latency_ms;
        Ok(link)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open(
        &mut self,
        device_name: Option<&str>,
        latency_ms: u32,
        play_now: bool,
    ) -> anyhow::Result<AudioLink> {
        let host = cpal::default_host();
        let device = match device_name {
            Some(wanted) => host
                .output_devices()
                .ok()
                .and_then(|mut devices| {
                    devices.find(|d| device_label(d).as_deref() == Some(wanted))
                })
                .or_else(|| host.default_output_device())
                .context("Failed to get an output device")?,
            None => host
                .default_output_device()
                .context("Failed to get default output device")?,
        };

        let supported_config = device.default_output_config()?;
        let channel_count = supported_config.channels() as usize;
        let mut config = supported_config.config();
        let sample_rate = config.sample_rate;

        let mut callback_frames = callback_frames(sample_rate);
        match supported_config.buffer_size() {
            cpal::SupportedBufferSize::Range { min, max } => {
                self.supported_buffer_size = (*min, *max);
                callback_frames = callback_frames.clamp(*min as usize, *max as usize);
                config.buffer_size = cpal::BufferSize::Fixed(callback_frames as u32);
            }
            cpal::SupportedBufferSize::Unknown => {
                self.supported_buffer_size = (0, 0);
                callback_frames = DEFAULT_UNKNOWN_CALLBACK_FRAMES;
            }
        }

        let target_frames = latency_frames(sample_rate, latency_ms).max(callback_frames);
        let (writer, mut reader) = queue::audio_queue(QueueConfig::new(sample_rate, target_frames));
        let stats = writer.stats().clone();
        let shared = self.shared.clone();

        let stream = device.build_output_stream(
            &config,
            move |data: &mut [f32], _| reader.read(data, channel_count, shared.gain()),
            |err| log::error!("Audio stream error: {err}"),
            None,
        )?;
        if play_now {
            stream.play()?;
        }

        self.stream = Some(stream);
        self.resumed = play_now;
        self.sample_rate = sample_rate;
        self.channel_count = channel_count;
        self.device_name = device_label(&device);
        self.device_description = device.description().ok();
        self.target_frames = target_frames;
        self.callback_frames = callback_frames;
        self.stats = stats;

        Ok(AudioLink {
            sample_rate,
            feed: AudioFeed::new(writer, sample_rate, target_frames),
        })
    }

    #[cfg(target_arch = "wasm32")]
    fn open(
        &mut self,
        _device_name: Option<&str>,
        latency_ms: u32,
        play_now: bool,
    ) -> anyhow::Result<AudioLink> {
        let output = web::WebOutput::open().map_err(|err| anyhow::anyhow!("{err:?}"))?;
        let sample_rate = output.sample_rate();
        let target_frames = latency_frames(sample_rate, latency_ms);
        let writer = output.connect(QueueConfig::new(sample_rate, target_frames));
        let stats = writer.stats().clone();
        output.set_gain(self.shared.gain());

        if play_now {
            output.resume().map_err(|err| anyhow::anyhow!("{err:?}"))?;
        }

        self.callback_frames = output.callback_frames();
        self.stream = Some(output);
        self.resumed = play_now;
        self.sample_rate = sample_rate;
        self.channel_count = 2;
        self.supported_buffer_size = (0, 0);
        self.device_name = None;
        self.device_description = None;
        self.target_frames = target_frames;
        self.stats = stats;

        Ok(AudioLink {
            sample_rate,
            feed: AudioFeed::new(writer, sample_rate, target_frames),
        })
    }

    pub fn resume(&mut self) {
        if self.resumed {
            return;
        }
        let Some(stream) = &self.stream else {
            return;
        };
        #[cfg(not(target_arch = "wasm32"))]
        let result = stream.play().map_err(|err| err.to_string());
        #[cfg(target_arch = "wasm32")]
        let result = stream.resume().map_err(|err| format!("{err:?}"));
        match result {
            Ok(()) => self.resumed = true,
            Err(err) => log::warn!("Failed to resume audio: {err}"),
        }
    }

    pub fn latency_ms(&self) -> u32 {
        self.latency_ms
    }

    pub fn target_frames(&self) -> usize {
        self.target_frames
    }

    pub fn callback_frames(&self) -> usize {
        #[cfg(target_arch = "wasm32")]
        if let Some(output) = &self.stream {
            return output.callback_frames();
        }
        self.callback_frames
    }

    pub fn device_name(&self) -> Option<&str> {
        self.device_name.as_deref()
    }

    pub fn requested_device(&self) -> Option<&str> {
        self.requested_device.as_deref()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn output_devices() -> Vec<String> {
        let Ok(devices) = cpal::default_host().output_devices() else {
            return Vec::new();
        };
        let mut names: Vec<String> = devices.filter_map(|d| device_label(&d)).collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    pub fn get_volume(&self) -> f32 {
        self.shared.volume.load()
    }

    pub fn set_volume(&self, volume: f32) {
        self.shared.volume.store(volume.clamp(0.0, 1.0));
        self.apply_gain();
    }

    pub fn is_muted(&self) -> bool {
        self.shared.muted.load(Ordering::Relaxed)
    }

    pub fn set_muted(&self, muted: bool) {
        self.shared.muted.store(muted, Ordering::Relaxed);
        self.apply_gain();
    }

    fn apply_gain(&self) {
        #[cfg(target_arch = "wasm32")]
        if let Some(output) = &self.stream {
            output.set_gain(self.shared.gain());
        }
    }

    pub fn get_sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn get_channel_count(&self) -> usize {
        self.channel_count
    }

    pub fn get_supported_buffer_size(&self) -> (u32, u32) {
        self.supported_buffer_size
    }

    pub fn stats(&self) -> &QueueStats {
        &self.stats
    }

    pub fn get_device_description(&self) -> &Option<cpal::DeviceDescription> {
        &self.device_description
    }

    pub fn reset_stats(&self) {
        self.stats.reset();
        #[cfg(target_arch = "wasm32")]
        if let Some(output) = &self.stream {
            output.reset_stats();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn callback_frames(sample_rate: u32) -> usize {
    ((sample_rate as f64 * CALLBACK_SECS) as usize).max(64)
}

#[cfg(not(target_arch = "wasm32"))]
fn device_label(device: &cpal::Device) -> Option<String> {
    device
        .description()
        .ok()
        .map(|desc| desc.name().to_string())
}

#[derive(Default)]
struct AtomicFloat(AtomicU32);

impl AtomicFloat {
    fn load(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    fn store(&self, val: f32) {
        self.0.store(val.to_bits(), Ordering::Relaxed);
    }
}

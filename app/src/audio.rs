use anyhow::Context;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::consumer::Consumer;
use ringbuf::producer::Producer;
use ringbuf::traits::{Observer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

#[cfg(target_arch = "wasm32")]
pub const MIN_LATENCY_MS: u32 = 40;
#[cfg(not(target_arch = "wasm32"))]
pub const MIN_LATENCY_MS: u32 = 20;

pub const MAX_LATENCY_MS: u32 = 250;

#[cfg(target_arch = "wasm32")]
pub const DEFAULT_LATENCY_MS: u32 = 90;
#[cfg(not(target_arch = "wasm32"))]
pub const DEFAULT_LATENCY_MS: u32 = 40;

#[cfg(target_arch = "wasm32")]
const MIN_CALLBACK_FRAMES: usize = 2048;

const DEFAULT_UNKNOWN_CALLBACK_FRAMES: usize = 1024;

fn default_callback_frames(target_frames: usize) -> usize {
    #[cfg(target_arch = "wasm32")]
    {
        (target_frames / 2).max(MIN_CALLBACK_FRAMES)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        (target_frames / 4).max(64)
    }
}

const BUFFER_CAP_QUARTERS: usize = 6;

const UNDERRUN_DECAY: f32 = 0.995;

#[derive(Default)]
struct Shared {
    volume: AtomicFloat,
    muted: AtomicBool,
    underrun_samples: AtomicU64,
    min_sample: AtomicFloat,
    max_sample: AtomicFloat,
}

pub struct AudioLink {
    pub sample_rate: u32,
    pub target_frames: usize,
    pub producer: HeapProd<f32>,
}

pub struct Audio {
    stream: Option<cpal::Stream>,
    shared: Arc<Shared>,
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
        shared.min_sample.store(f32::INFINITY);
        shared.max_sample.store(f32::NEG_INFINITY);

        Self {
            stream: None,
            shared: Arc::new(shared),
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

        #[cfg(target_arch = "wasm32")]
        if let Some(rate) = hardware_sample_rate() {
            config.sample_rate = rate;
        }

        let sample_rate = config.sample_rate;

        let latency_ms = latency_ms.clamp(MIN_LATENCY_MS, MAX_LATENCY_MS);
        let target_frames = (sample_rate as usize * latency_ms as usize / 1000).max(1);

        let mut callback_frames = default_callback_frames(target_frames);
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

        let queue_target = target_frames.max(callback_frames);
        let ring_frames = (queue_target * BUFFER_CAP_QUARTERS / 4).max(callback_frames * 3);

        let ring = HeapRb::<f32>::new(ring_frames * 2);
        let (producer, mut consumer) = ring.split();

        let shared = self.shared.clone();
        shared.underrun_samples.store(0, Ordering::Relaxed);

        let mut held = [0.0f32; 2];

        let stream = device.build_output_stream(
            &config,
            move |data: &mut [f32], _| {
                write_samples(data, channel_count, &mut consumer, &shared, &mut held)
            },
            |err| log::error!("Audio stream error: {err}"),
            None,
        )?;

        let play_now = !cfg!(target_arch = "wasm32") || self.user_activated;
        if play_now {
            stream.play()?;
        }

        self.stream = Some(stream);
        self.resumed = play_now;
        self.sample_rate = sample_rate;
        self.channel_count = channel_count;
        self.device_name = device_label(&device);
        self.device_description = device.description().ok();
        self.requested_device = device_name.map(str::to_owned);
        self.latency_ms = latency_ms;
        self.target_frames = queue_target;
        self.callback_frames = callback_frames;

        Ok(AudioLink {
            sample_rate,
            target_frames: queue_target,
            producer,
        })
    }

    pub fn resume(&mut self) {
        if self.resumed {
            return;
        }
        let Some(stream) = &self.stream else {
            return;
        };
        match stream.play() {
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
        self.callback_frames
    }

    pub fn device_name(&self) -> Option<&str> {
        self.device_name.as_deref()
    }

    pub fn requested_device(&self) -> Option<&str> {
        self.requested_device.as_deref()
    }

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
    }

    pub fn is_muted(&self) -> bool {
        self.shared.muted.load(Ordering::Relaxed)
    }

    pub fn set_muted(&self, muted: bool) {
        self.shared.muted.store(muted, Ordering::Relaxed);
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

    pub fn get_underrun_samples(&self) -> u64 {
        self.shared.underrun_samples.load(Ordering::Relaxed)
    }

    pub fn get_min_sample(&self) -> f32 {
        self.shared.min_sample.load()
    }

    pub fn get_max_sample(&self) -> f32 {
        self.shared.max_sample.load()
    }

    pub fn get_device_description(&self) -> &Option<cpal::DeviceDescription> {
        &self.device_description
    }

    pub fn reset_stats(&self) {
        self.shared.underrun_samples.store(0, Ordering::Relaxed);
        self.shared.min_sample.store(f32::INFINITY);
        self.shared.max_sample.store(f32::NEG_INFINITY);
    }
}

#[cfg(target_arch = "wasm32")]
fn hardware_sample_rate() -> Option<u32> {
    use std::sync::OnceLock;
    static RATE: OnceLock<Option<u32>> = OnceLock::new();

    *RATE.get_or_init(|| {
        let ctx = web_sys::AudioContext::new().ok()?;
        let rate = ctx.sample_rate();
        let _ = ctx.close();

        (rate.is_finite() && (8_000.0..=96_000.0).contains(&rate)).then_some(rate as u32)
    })
}

fn device_label(device: &cpal::Device) -> Option<String> {
    device
        .description()
        .ok()
        .map(|desc| desc.name().to_string())
}

fn write_samples(
    data: &mut [f32],
    channels: usize,
    consumer: &mut HeapCons<f32>,
    shared: &Shared,
    held: &mut [f32; 2],
) {
    let gain = if shared.muted.load(Ordering::Relaxed) {
        0.0
    } else {
        let volume = shared.volume.load();
        volume * volume
    };

    let mut underruns = 0u64;
    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;

    if channels == 0 {
        return;
    }

    for out in data.chunks_mut(channels) {
        let mut frame = [0.0f32; 2];
        if consumer.pop_slice(&mut frame) == 2 {
            *held = frame;
        } else {
            held[0] *= UNDERRUN_DECAY;
            held[1] *= UNDERRUN_DECAY;
            frame = *held;
            underruns += 1;
        }

        min = min.min(frame[0]).min(frame[1]);
        max = max.max(frame[0]).max(frame[1]);

        match channels {
            1 => out[0] = (frame[0] + frame[1]) * 0.5 * gain,
            _ => {
                out[0] = frame[0] * gain;
                out[1] = frame[1] * gain;
                for extra in &mut out[2..] {
                    *extra = 0.0;
                }
            }
        }
    }

    if underruns > 0 {
        shared
            .underrun_samples
            .fetch_add(underruns, Ordering::Relaxed);
    }
    if min < shared.min_sample.load() {
        shared.min_sample.store(min);
    }
    if max > shared.max_sample.load() {
        shared.max_sample.store(max);
    }
}

pub fn push_frames(producer: &mut HeapProd<f32>, samples: &[f32]) -> usize {
    let room = producer.vacant_len() & !1;
    let take = room.min(samples.len() & !1);
    producer.push_slice(&samples[..take])
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

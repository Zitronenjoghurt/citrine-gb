use anyhow::Context;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::consumer::Consumer;
use ringbuf::traits::Split;
use ringbuf::{HeapCons, HeapProd, HeapRb};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

pub struct Audio {
    pub stream: Option<cpal::Stream>,
    underrun_samples: Arc<AtomicU64>,
    pending_consumer: Option<HeapCons<f32>>,
    volume: Arc<AtomicFloat>,
    sample_rate: u32,
    channel_count: usize,
    supported_buffer_size: (u32, u32),
    min_sample: Arc<AtomicFloat>,
    max_sample: Arc<AtomicFloat>,
    device_description: Option<cpal::DeviceDescription>,
}

impl Audio {
    pub fn new() -> (Self, HeapProd<f32>) {
        let ring_buffer = HeapRb::<f32>::new(8192);
        let (producer, consumer) = ring_buffer.split();

        let audio = Self {
            stream: None,
            underrun_samples: Arc::new(AtomicU64::new(0)),
            pending_consumer: Some(consumer),
            volume: Arc::new(AtomicFloat::new(1.0)),
            sample_rate: 44100,
            channel_count: 0,
            supported_buffer_size: (0, 0),
            min_sample: Arc::new(AtomicFloat::new(f32::INFINITY)),
            max_sample: Arc::new(AtomicFloat::new(f32::NEG_INFINITY)),
            device_description: None,
        };

        (audio, producer)
    }

    pub fn try_start(&mut self) -> anyhow::Result<u32> {
        if self.stream.is_some() {
            return Ok(self.sample_rate);
        };

        let Some(mut consumer) = self.pending_consumer.take() else {
            return Err(anyhow::anyhow!("Audio consumer missing or already taken"));
        };

        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .context("Failed to get default output device")?;
        self.channel_count = device.default_output_config()?.channels() as usize;
        self.device_description = Some(device.description()?);

        let supported_config = device.default_output_config()?;
        let sample_rate = supported_config.sample_rate() as u32;
        let mut config = supported_config.config();

        match supported_config.buffer_size() {
            cpal::SupportedBufferSize::Range { min, max } => {
                self.supported_buffer_size = (*min, *max);
                let target_size = 1024.clamp(*min, *max);
                config.buffer_size = cpal::BufferSize::Fixed(target_size);
            }
            cpal::SupportedBufferSize::Unknown => {}
        }

        self.sample_rate = sample_rate;
        let volume = self.volume.clone();
        let underrun_counter = self.underrun_samples.clone();
        let min_sample = self.min_sample.clone();
        let max_sample = self.max_sample.clone();

        let stream = device.build_output_stream(
            &config,
            move |data: &mut [f32], _| {
                let current_vol = volume.load();
                let mut local_underruns = 0;

                for sample in data.iter_mut() {
                    match consumer.try_pop() {
                        Some(s) => {
                            *sample = s * current_vol;
                            if s < min_sample.load() {
                                min_sample.store(s);
                            }
                            if s > max_sample.load() {
                                max_sample.store(s);
                            }
                        }
                        None => {
                            *sample = 0.0;
                            local_underruns += 1;
                        }
                    }
                }

                if local_underruns > 0 {
                    underrun_counter.fetch_add(local_underruns, Ordering::Relaxed);
                }
            },
            |err| eprintln!("Audio stream error: {}", err),
            None,
        )?;

        stream.play()?;
        self.stream = Some(stream);

        Ok(sample_rate)
    }

    pub fn get_volume(&self) -> f32 {
        self.volume.load()
    }

    pub fn set_volume(&self, volume: f32) {
        self.volume.store(volume);
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
        self.underrun_samples.load(Ordering::Relaxed)
    }

    pub fn get_min_sample(&self) -> f32 {
        self.min_sample.load()
    }

    pub fn get_max_sample(&self) -> f32 {
        self.max_sample.load()
    }

    pub fn get_device_description(&self) -> &Option<cpal::DeviceDescription> {
        &self.device_description
    }
}

struct AtomicFloat(AtomicU32);

impl AtomicFloat {
    fn new(val: f32) -> Self {
        Self(AtomicU32::new(val.to_bits()))
    }

    fn load(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    fn store(&self, val: f32) {
        self.0.store(val.to_bits(), Ordering::Relaxed);
    }
}

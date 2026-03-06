use anyhow::Context;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::consumer::Consumer;
use ringbuf::traits::Split;
use ringbuf::{HeapCons, HeapProd, HeapRb};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

pub struct Audio {
    pub stream: Option<cpal::Stream>,
    pending_consumer: Option<HeapCons<f32>>,
    volume: Arc<AtomicVolume>,
    sample_rate: u32,
}

impl Audio {
    pub fn new() -> (Self, HeapProd<f32>) {
        let ring_buffer = HeapRb::<f32>::new(8192);
        let (producer, consumer) = ring_buffer.split();

        let audio = Self {
            stream: None,
            pending_consumer: Some(consumer),
            volume: Arc::new(AtomicVolume::new(1.0)),
            sample_rate: 44100,
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

        let supported_config = device.default_output_config()?;
        let sample_rate = supported_config.sample_rate() as u32;
        let config = supported_config.config();

        self.sample_rate = sample_rate;
        let volume = self.volume.clone();

        let stream = device.build_output_stream(
            &config,
            move |data: &mut [f32], _| {
                let current_vol = volume.load();
                for sample in data.iter_mut() {
                    *sample = consumer.try_pop().unwrap_or(0.0) * current_vol;
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
}

struct AtomicVolume(AtomicU32);

impl AtomicVolume {
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

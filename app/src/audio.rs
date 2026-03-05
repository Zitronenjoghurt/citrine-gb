use anyhow::Context;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::consumer::Consumer;
use ringbuf::HeapCons;

pub fn init_audio(mut consumer: HeapCons<f32>) -> anyhow::Result<(cpal::Stream, u32)> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .context("Failed to get default output device")?;

    let supported_config = device.default_output_config()?;
    let sample_rate = supported_config.sample_rate() as u32;
    let config = supported_config.config();

    let stream = device.build_output_stream(
        &config,
        move |data: &mut [f32], _| {
            for sample in data.iter_mut() {
                *sample = consumer.try_pop().unwrap_or(0.0);
            }
        },
        |err| eprintln!("Audio stream error: {}", err),
        None,
    )?;

    stream.play()?;

    Ok((stream, sample_rate))
}

mod common;

use citrine_gb_nds_gpu::{
    Antialiasing, FIRST_PASS_STAMP, Mipmaps, Pass, Renderer, Textures, Upscale,
};
use common::scenes;

fn stamped_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok();
    let Some(adapter) = adapter else {
        assert!(
            std::env::var_os("CITRINE_GPU_REQUIRED").is_none(),
            "no GPU adapter, and CITRINE_GPU_REQUIRED is set"
        );
        return None;
    };
    if !adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
        eprintln!("no timestamp queries, skipped");
        return None;
    }
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: wgpu::Features::TIMESTAMP_QUERY,
        ..Default::default()
    }))
    .ok()
}

fn read_stamps(device: &wgpu::Device, queue: &wgpu::Queue, query_set: &wgpu::QuerySet) -> Vec<u64> {
    let count = FIRST_PASS_STAMP + Pass::ALL.len() as u32;
    let bytes = u64::from(count) * 8;
    let resolve = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: bytes,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: bytes,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.resolve_query_set(query_set, 0..count, &resolve, 0);
    encoder.copy_buffer_to_buffer(&resolve, 0, &read, 0, bytes);
    queue.submit([encoder.finish()]);
    read.slice(..)
        .map_async(wgpu::MapMode::Read, |result| result.expect("map"));
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("wait");
    let view = read.slice(..).get_mapped_range();
    let (words, _) = view.as_chunks::<8>();
    words.iter().map(|word| u64::from_le_bytes(*word)).collect()
}

#[test]
fn the_frame_starts_before_the_mipmaps_pass() {
    let Some((device, queue)) = stamped_device() else {
        return;
    };
    let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
        label: None,
        ty: wgpu::QueryType::Timestamp,
        count: FIRST_PASS_STAMP + Pass::ALL.len() as u32,
    });
    let mut renderer = Renderer::new(&device);
    renderer.stamp_passes(Some(query_set.clone()));
    let scene = scenes::textures();
    let upscale = Upscale {
        scale: 4,
        antialiasing: Antialiasing::Off,
        textures: Textures::Nearest,
        mipmaps: Mipmaps::Anisotropic4x,
    };
    let commands = renderer.encode(
        &device,
        &queue,
        &scene.frame,
        &scene.registers,
        &scene.memory,
        upscale,
    );
    queue.submit([commands]);
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("wait");
    let passes = renderer.passes();
    assert_eq!(passes.first(), Some(&Pass::Mipmaps));
    let words = read_stamps(&device, &queue, &query_set);
    let ends = &words[FIRST_PASS_STAMP as usize..][..passes.len()];
    assert!(
        words[0] != 0 && words[0] <= ends[0],
        "frame start {} after the mipmaps end {}",
        words[0],
        ends[0]
    );
    assert!(
        ends.windows(2).all(|pair| pair[0] <= pair[1]),
        "{passes:?} ended out of order: {ends:?}"
    );
}

mod script;

use citrine_gb::nds::machine::{Nds, NdsFirmware};
use citrine_gb_nds_gpu::{Antialiasing, Picture, Renderer, Upscale};
use std::path::Path;
use std::time::Instant;

const BOOT: usize = 240;
const WARMUP: usize = 30;
const FRAMES: usize = 300;
const BURST: usize = 120;

const CHOICES: [(&str, u32, Antialiasing); 4] = [
    ("native", 1, Antialiasing::Off),
    ("4x", 4, Antialiasing::Off),
    ("4x On", 4, Antialiasing::Four),
    ("4x More", 4, Antialiasing::Eight),
];

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    assert!(!paths.is_empty(), "usage: gpu_timing <rom>...");
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("a GPU adapter");
    println!("adapter: {}", adapter.get_info().name);
    let pictures = std::env::var_os("GPU_TIMING_PNG");
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .expect("a GPU device");
    let boot = std::env::var("GPU_TIMING_BOOT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(BOOT);
    let steps = script::parse(&std::env::var("GPU_TIMING_SCRIPT").unwrap_or_default());
    for path in paths {
        let rom = std::fs::read(&path).expect("read the rom");
        let name = std::path::Path::new(&path)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or(path.clone());
        let mut nds = Nds::new(rom.as_slice().into(), NdsFirmware::default()).expect("boots");
        for frame in 0..boot {
            nds.set_keys(script::keys_at(&steps, frame));
            nds.run_frame();
        }
        let mut renderers: Vec<Renderer> = CHOICES.iter().map(|_| Renderer::new(&device)).collect();
        let mut times = vec![Vec::with_capacity(FRAMES); CHOICES.len()];
        let mut polygons = 0;
        for frame in 0..WARMUP + FRAMES {
            nds.set_keys(script::keys_at(&steps, boot + frame));
            nds.run_frame();
            let scene = nds.three_d_scene();
            polygons = polygons.max(scene.frame.polygons.len());
            for ((renderer, times), (_, scale, antialiasing)) in
                renderers.iter_mut().zip(&mut times).zip(CHOICES)
            {
                let start = Instant::now();
                renderer.render(
                    &device,
                    &queue,
                    scene.frame,
                    &scene.registers,
                    scene.textures,
                    upscale(scale, antialiasing),
                );
                device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .expect("wait for the frame");
                if frame >= WARMUP {
                    times.push(start.elapsed().as_secs_f64() * 1000.0);
                }
            }
        }
        let scene = nds.three_d_scene();
        for ((renderer, mut times), (label, scale, antialiasing)) in
            renderers.iter_mut().zip(times).zip(CHOICES)
        {
            let start = Instant::now();
            for _ in 0..BURST {
                renderer.render(
                    &device,
                    &queue,
                    scene.frame,
                    &scene.registers,
                    scene.textures,
                    upscale(scale, antialiasing),
                );
            }
            device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("wait for the burst");
            let burst = start.elapsed().as_secs_f64() * 1000.0 / BURST as f64;
            if let Some(dir) = &pictures {
                let file = format!("{name}-{}.png", label.replace(' ', "-"));
                save_png(&Path::new(dir).join(file), &renderer.read(&device, &queue));
            }
            times.sort_by(f64::total_cmp);
            let mean = times.iter().sum::<f64>() / times.len() as f64;
            println!(
                "{name} | {label} | median {:.2} ms | mean {mean:.2} ms | p95 {:.2} ms | back to back {burst:.2} ms | {polygons} polygons",
                times[times.len() / 2],
                times[times.len() * 95 / 100],
            );
        }
    }
}

fn upscale(scale: u32, antialiasing: Antialiasing) -> Upscale {
    Upscale {
        scale,
        antialiasing,
        ..Upscale::default()
    }
}

fn save_png(path: &Path, picture: &Picture) {
    let level =
        |value: u8, alpha: u8| (u32::from(value) * 255 * u32::from(alpha) / (63 * 31)) as u8;
    let bytes: Vec<u8> = (0..picture.height())
        .flat_map(|y| (0..picture.width()).map(move |x| picture.pixel(x, y)))
        .flat_map(|pixel| {
            [
                level(pixel.r, pixel.alpha),
                level(pixel.g, pixel.alpha),
                level(pixel.b, pixel.alpha),
                255,
            ]
        })
        .collect();
    let file = std::fs::File::create(path).expect("create the picture");
    let mut encoder = png::Encoder::new(
        std::io::BufWriter::new(file),
        picture.width() as u32,
        picture.height() as u32,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("write the header");
    writer.write_image_data(&bytes).expect("write the picture");
}

mod script;

use citrine_gb::nds::machine::{Nds, NdsFirmware};
use citrine_gb::nds::render::Pixel;
use citrine_gb::nds::video::{Composite, HEIGHT, WIDTH, color};
use citrine_gb_nds_gpu::{Antialiasing, Renderer, Upscale};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const WINDOW: usize = 60;

fn frames_list(name: &str) -> Vec<usize> {
    std::env::var(name)
        .unwrap_or_default()
        .split(',')
        .filter_map(|frame| frame.parse().ok())
        .collect()
}

#[derive(Default)]
struct Counts {
    frames: usize,
    without_3d: usize,
    upscaled: usize,
    finished: usize,
    scrolled: usize,
    fallbacks: BTreeMap<String, usize>,
}

impl Counts {
    fn add(&mut self, composite: &Composite, finishes: bool) {
        self.frames += 1;
        match composite.fallback() {
            Some(reason) => *self.fallbacks.entry(format!("{reason:?}")).or_default() += 1,
            None if !composite.shows_3d() => self.without_3d += 1,
            None => {
                self.upscaled += 1;
                self.finished += usize::from(finishes);
                self.scrolled += usize::from((0..HEIGHT).any(|y| composite.scroll(y) != 0));
            }
        }
    }

    fn line(&self, label: &str) -> String {
        let fallbacks: Vec<String> = self
            .fallbacks
            .iter()
            .map(|(reason, count)| format!("{reason} {count}"))
            .collect();
        format!(
            "{label} | frames {} | no 3d {} | upscaled {} | edge or fog {} | scrolled {} | fallbacks {}",
            self.frames,
            self.without_3d,
            self.upscaled,
            self.finished,
            self.scrolled,
            if fallbacks.is_empty() {
                "none".to_owned()
            } else {
                fallbacks.join(", ")
            }
        )
    }
}

fn widen(color: u32) -> [u32; 3] {
    [color & 63, (color >> 6) & 63, (color >> 12) & 63]
}

fn composed(composite: &Composite, picture: &citrine_gb_nds_gpu::Picture) -> Vec<u8> {
    let scale = picture.scale() as usize;
    let mut out = Vec::with_capacity(WIDTH * HEIGHT * scale * scale * 4);
    for y in 0..HEIGHT * scale {
        for x in 0..WIDTH * scale {
            let layer = composite.pixels()[y / scale * WIDTH + x / scale];
            let source = (x + usize::from(composite.scroll(y / scale)) * scale) % (512 * scale);
            let up = (source < WIDTH * scale)
                .then(|| picture.pixel(source, y))
                .filter(|pixel| pixel.alpha != 0);
            let three_d = up.map(|pixel| {
                let Pixel { r, g, b, alpha } = pixel;
                (color::from_channels(r, g, b), alpha)
            });
            let color = widen(layer.finish(three_d, composite.master(y / scale)));
            out.extend(color.map(|c| ((c << 2) | (c >> 4)) as u8));
            out.push(255);
        }
    }
    out
}

fn save_png(path: &Path, width: usize, height: usize, rgba: &[u8]) {
    let file = std::fs::File::create(path).expect("create the picture");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("write the header");
    writer.write_image_data(rgba).expect("write the picture");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert!(
        args.len() >= 4,
        "usage: fallbacks <rom> <frames> <out> [script]"
    );
    let rom = std::fs::read(&args[1]).expect("read the rom");
    let frames: usize = args[2].parse().expect("a frame count");
    let out = PathBuf::from(&args[3]);
    let steps = script::parse(args.get(4).map_or("", String::as_str));
    let every: usize = std::env::var("SHOT_EVERY")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(60);
    let start: usize = std::env::var("COUNT_FROM")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let pictures = frames_list("PICTURES");
    std::fs::create_dir_all(&out).expect("make the output folder");
    let mut nds = Nds::new(rom.into(), NdsFirmware::default()).expect("the rom boots");
    if let Ok(path) = std::env::var("SAVE_IN") {
        let bytes = std::fs::read(path).expect("read the save");
        nds.buses_mut().cart_mut().aux_mut().load(&bytes);
    }
    nds.record_composite(true);
    let gpu = (!pictures.is_empty()).then(|| {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .expect("a GPU adapter");
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .expect("a GPU device")
    });
    let mut total = Counts::default();
    let mut window = Counts::default();
    for frame in 0..frames {
        nds.set_keys(script::keys_at(&steps, frame));
        nds.run_frame();
        let composite = nds.composite().expect("the composite is recorded");
        let finishes = nds.three_d_scene().registers.marks_edges_or_fogs();
        if frame >= start {
            total.add(composite, finishes);
            window.add(composite, finishes);
        }
        if (frame + 1) % WINDOW == 0 && window.frames > 0 {
            println!(
                "{}",
                window.line(&format!("{:05}-{frame:05}", frame + 1 - WINDOW))
            );
            window = Counts::default();
        }
        if frame % every == 0 || frame + 1 == frames {
            save_png(
                &out.join(format!("{frame:05}.png")),
                WIDTH,
                HEIGHT * 2,
                nds.frame(),
            );
        }
        if let Some((device, queue)) = gpu.as_ref().filter(|_| pictures.contains(&frame)) {
            println!("picture {frame}: fallback {:?}", composite.fallback());
            let scene = nds.three_d_scene();
            for scale in [1, 4] {
                let mut renderer = Renderer::new(device);
                let upscale = Upscale {
                    scale,
                    antialiasing: Antialiasing::Off,
                    ..Upscale::default()
                };
                renderer.render(
                    device,
                    queue,
                    scene.frame,
                    &scene.registers,
                    scene.textures,
                    upscale,
                );
                let picture = renderer.read(device, queue);
                let side = scale as usize;
                save_png(
                    &out.join(format!("{frame:05}-{scale}x.png")),
                    WIDTH * side,
                    HEIGHT * side,
                    &composed(composite, &picture),
                );
            }
        }
    }
    println!("{}", total.line("total"));
}

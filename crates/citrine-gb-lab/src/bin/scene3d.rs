use anyhow::{Context, bail};
use citrine_gb::nds::render::Pixel;
use citrine_gb::nds::video::{HEIGHT, WIDTH};
use citrine_gb_nds_gpu::{Antialiasing, Picture, Renderer as GpuRenderer, Upscale};
use citrine_lab::emulator::FrameEmulator;
use citrine_lab::emulators::{DsBios, MelonDsEmulator};
use citrine_lab::scene3d::{Difference, Layer, Scene, rgba, same};
use clap::{Parser, Subcommand};
use melonds_sys as sys;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(about = "Dump DS 3D frames from melonDS and replay them through citrine's renderer")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Dump {
        #[arg(long)]
        rom: PathBuf,
        #[arg(long)]
        script: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    Replay {
        #[arg(long)]
        dir: PathBuf,
        #[arg(long)]
        pictures: Option<PathBuf>,
        #[arg(long, default_value_t = false)]
        gpu: bool,
        #[arg(long, default_value_t = 0)]
        detail: usize,
        #[arg(long, value_delimiter = ',', num_args = 2)]
        probe: Vec<i32>,
    },
}

fn main() -> anyhow::Result<()> {
    match Args::parse().command {
        Command::Dump { rom, script, out } => dump(&rom, &script, &out),
        Command::Replay {
            dir,
            pictures,
            gpu,
            detail,
            probe,
        } => {
            if let [x, y] = probe[..] {
                return self::probe(&dir, x, y);
            }
            replay(&dir, pictures.as_deref(), gpu, detail)
        }
    }
}

fn keys(text: &str) -> anyhow::Result<u32> {
    let mut keys = 0;
    for name in text.split('+') {
        keys |= match name.to_ascii_uppercase().as_str() {
            "A" => sys::KEY_A,
            "B" => sys::KEY_B,
            "X" => sys::KEY_X,
            "Y" => sys::KEY_Y,
            "L" => sys::KEY_L,
            "R" => sys::KEY_R,
            "START" => sys::KEY_START,
            "SELECT" => sys::KEY_SELECT,
            "UP" => sys::KEY_UP,
            "DOWN" => sys::KEY_DOWN,
            "LEFT" => sys::KEY_LEFT,
            "RIGHT" => sys::KEY_RIGHT,
            "NONE" => 0,
            other => bail!("unknown key {other}"),
        };
    }
    Ok(keys)
}

struct Run {
    emu: MelonDsEmulator,
    frames: u64,
}

impl Run {
    fn frames(&mut self, keys: u32, count: u64) {
        self.emu.set_keys(keys);
        for _ in 0..count {
            self.emu.step();
            self.frames += 1;
        }
    }
}

fn dump(rom_path: &Path, script_path: &Path, out: &Path) -> anyhow::Result<()> {
    let rom = std::fs::read(rom_path).with_context(|| format!("reading {}", rom_path.display()))?;
    let script = std::fs::read_to_string(script_path)?;
    std::fs::create_dir_all(out)?;
    let mut emu = MelonDsEmulator::new();
    emu.load_nds(&rom, DsBios::FREE)?;
    let mut run = Run { emu, frames: 0 };
    let mut scene = Scene::zeroed();
    for (number, line) in script.lines().enumerate() {
        let words: Vec<&str> = line.split_whitespace().collect();
        let number = number + 1;
        let count = |index: usize, default: u64| -> anyhow::Result<u64> {
            words
                .get(index)
                .map_or(Ok(default), |word| word.parse())
                .with_context(|| format!("line {number}: bad number"))
        };
        match words.as_slice() {
            [] => {}
            [first, ..] if first.starts_with('#') => {}
            ["wait", ..] => run.frames(0, count(1, 1)?),
            ["hold", held, ..] => run.frames(keys(held)?, count(2, 1)?),
            ["tap", held, ..] => {
                let held = keys(held)?;
                let gap = count(3, 24)?;
                for _ in 0..count(2, 1)? {
                    run.frames(held, 6);
                    run.frames(0, gap);
                }
            }
            ["touch", x, y, ..] => {
                let point = (x.parse()?, y.parse()?);
                run.emu.touch(Some(point));
                run.frames(0, count(3, 6)?);
                run.emu.touch(None);
                run.frames(0, count(4, 30)?);
            }
            ["shot", name] => {
                screens(&run.emu, &out.join(format!("{name}.png")))?;
                println!("{name}: frame {}", run.frames);
            }
            ["dump", name] => {
                run.emu.scene3d(&mut scene);
                scene.save(&out.join(format!("{name}.scene")))?;
                screens(&run.emu, &out.join(format!("{name}.png")))?;
                let raw = scene.raw();
                println!(
                    "{name}: frame {} polygons {} vertices {} disp3dcnt {:#06x}",
                    run.frames, raw.polygon_count, raw.vertex_count, raw.disp3dcnt
                );
            }
            _ => bail!("line {number}: cannot read {line:?}"),
        }
    }
    println!("ran {} frames", run.frames);
    Ok(())
}

fn screens(emu: &MelonDsEmulator, path: &Path) -> anyhow::Result<()> {
    let mut rgba = Vec::new();
    emu.render_into(&mut rgba);
    write_png(path, WIDTH, HEIGHT * 2, &rgba)
}

fn write_png(path: &Path, width: usize, height: usize, rgba: &[u8]) -> anyhow::Result<()> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(rgba)?;
    Ok(())
}

fn picture(width: usize, height: usize, pixel: impl Fn(usize, usize) -> Pixel) -> Vec<u8> {
    let mut out = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        for x in 0..width {
            out.extend_from_slice(&rgba(pixel(x, y)));
        }
    }
    out
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: GpuRenderer,
}

impl Gpu {
    fn open() -> anyhow::Result<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;
        let renderer = GpuRenderer::new(&device);
        Ok(Self {
            device,
            queue,
            renderer,
        })
    }

    fn render(&mut self, scene: &Scene, layer: Layer, upscale: Upscale) -> Picture {
        self.renderer.render(
            &self.device,
            &self.queue,
            &scene.frame(),
            &scene.registers(layer),
            &scene.memory(),
            upscale,
        );
        self.renderer.read(&self.device, &self.queue)
    }
}

fn probe(dir: &Path, x: i32, y: i32) -> anyhow::Result<()> {
    for path in scenes(dir)? {
        let scene = Scene::load(&path)?;
        let frame = scene.frame();
        println!("{}:", path.display());
        for (index, polygon) in frame.polygons.iter().enumerate() {
            let points: Vec<[i32; 2]> = (0..usize::from(polygon.count))
                .map(|slot| frame.vertex(polygon, slot).screen.map(i32::from))
                .collect();
            let low = points
                .iter()
                .fold([i32::MAX; 2], |a, p| [a[0].min(p[0]), a[1].min(p[1])]);
            let high = points
                .iter()
                .fold([i32::MIN; 2], |a, p| [a[0].max(p[0]), a[1].max(p[1])]);
            if (low[0]..=high[0]).contains(&x) && (low[1]..=high[1]).contains(&y) {
                println!(
                    "  #{index} attr {:#010x} tex {:#010x} front {} top {} bottom {} points {points:?} depth {:?}",
                    polygon.attributes,
                    polygon.texture,
                    polygon.front,
                    polygon.top,
                    polygon.bottom,
                    &polygon.depth[..usize::from(polygon.count)]
                );
            }
        }
    }
    Ok(())
}

fn scenes(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "scene"))
        .collect();
    paths.sort();
    Ok(paths)
}

fn report(label: &str, difference: Difference) -> String {
    format!(
        " {label} {} px (alpha {}, first {:?})",
        difference.pixels, difference.alpha, difference.first
    )
}

fn replay(dir: &Path, pictures: Option<&Path>, gpu: bool, detail: usize) -> anyhow::Result<()> {
    let paths = scenes(dir)?;
    let mut gpu = if gpu { Some(Gpu::open()?) } else { None };
    if let Some(pictures) = pictures {
        std::fs::create_dir_all(pictures)?;
    }
    for path in paths {
        let name = path.file_stem().unwrap_or_default().to_string_lossy();
        let scene = Scene::load(&path)?;
        let raw = scene.raw();
        let mut line = format!(
            "{name}: polygons {} disp3dcnt {:#06x}",
            raw.polygon_count, raw.disp3dcnt
        );
        let cpu = scene.render_cpu(Layer::Final);
        for layer in [Layer::Plain, Layer::Final] {
            let renderer = scene.render_cpu(layer);
            let label = format!("cpu-{layer:?}").to_lowercase();
            line += &report(&label, scene.difference(layer, |x, y| renderer.pixel(x, y)));
            let mut shown = 0;
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    let (ours, theirs) = (renderer.pixel(x, y), scene.melonds(layer, x, y));
                    if shown < detail && !same(ours, theirs) {
                        shown += 1;
                        let raw = scene.raw();
                        let at = y * WIDTH + x;
                        line += &format!(
                            "\n  {layer:?} ({x}, {y}) ours {ours:?} {:?} depth {:#x} under {:?}\n    melonds {theirs:?} attr {:#010x} depth {:#x} under color {:#010x} attr {:#010x} depth {:#x}",
                            renderer.attribute(x, y),
                            renderer.depth(x, y),
                            renderer.under(x, y),
                            raw.attributes[at],
                            raw.depth[at],
                            raw.under_color[at],
                            raw.under_attributes[at],
                            raw.under_depth[at],
                        );
                    }
                }
            }
        }
        let mut upscaled = Vec::new();
        if let Some(gpu) = gpu.as_mut() {
            for layer in [Layer::Plain, Layer::Final] {
                let one = gpu.render(&scene, layer, Upscale::default());
                let label = format!("gpu-{layer:?}").to_lowercase();
                line += &report(&label, scene.difference(layer, |x, y| one.pixel(x, y)));
            }
            for (label, scale, antialiasing) in [
                ("1x", 1, Antialiasing::Off),
                ("2x", 2, Antialiasing::Off),
                ("4x", 4, Antialiasing::Off),
                ("8x", 8, Antialiasing::Off),
                ("aa", 4, Antialiasing::Four),
            ] {
                let picture = gpu.render(
                    &scene,
                    Layer::Final,
                    Upscale {
                        scale,
                        antialiasing,
                        ..Upscale::default()
                    },
                );
                upscaled.push((label, picture));
            }
        }
        println!("{line}");
        let Some(pictures) = pictures else {
            continue;
        };
        let ours = picture(WIDTH, HEIGHT, |x, y| cpu.pixel(x, y));
        let theirs = picture(WIDTH, HEIGHT, |x, y| scene.melonds(Layer::Final, x, y));
        let marked = picture(WIDTH, HEIGHT, |x, y| {
            let (a, b) = (cpu.pixel(x, y), scene.melonds(Layer::Final, x, y));
            if !same(a, b) {
                Pixel {
                    r: 63,
                    g: 0,
                    b: 63,
                    alpha: 31,
                }
            } else {
                Pixel {
                    r: a.r / 3,
                    g: a.g / 3,
                    b: a.b / 3,
                    alpha: 31,
                }
            }
        });
        for (label, pixels) in [("cpu", &ours), ("melonds", &theirs), ("diff", &marked)] {
            write_png(
                &pictures.join(format!("{name}-{label}.png")),
                WIDTH,
                HEIGHT,
                pixels,
            )?;
        }
        for (label, picture) in &upscaled {
            let (width, height) = (picture.width(), picture.height());
            let pixels = self::picture(width, height, |x, y| picture.pixel(x, y));
            write_png(
                &pictures.join(format!("{name}-gpu-{label}.png")),
                width,
                height,
                &pixels,
            )?;
        }
    }
    Ok(())
}

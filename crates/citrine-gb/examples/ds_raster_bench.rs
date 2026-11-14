use citrine_gb::console::{Console, Keys};
use citrine_gb::nds::geometry::frame::Frame;
use citrine_gb::nds::machine::{Nds, NdsFirmware};
use citrine_gb::nds::render::{Registers, Renderer, Stencil};
use citrine_gb::nds::three_d::Textures;
use citrine_gb::nds::video::{HEIGHT, Line3d, WIDTH};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Instant;

struct Scene {
    frame: Frame,
    registers: Registers,
    textures: Textures,
    set: usize,
}

impl Scene {
    fn digest(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.frame.hash(&mut hasher);
        format!("{:?}", self.registers).hash(&mut hasher);
        self.textures.image().hash(&mut hasher);
        self.textures.palette().hash(&mut hasher);
        hasher.finish()
    }
}

struct Run {
    scenes: Vec<Scene>,
    digests: Vec<u64>,
}

impl Run {
    fn new(scenes: Vec<Scene>) -> Self {
        let digests = scenes.iter().map(Scene::digest).collect();
        Self { scenes, digests }
    }

    fn save(&self, path: &str) {
        let mut saved = Saved {
            textures: Vec::new(),
            scenes: Vec::new(),
        };
        for scene in &self.scenes {
            let same = saved.textures.last().is_some_and(|last| {
                last.image() == scene.textures.image() && last.palette() == scene.textures.palette()
            });
            if !same {
                saved.textures.push(scene.textures.clone());
            }
            saved.scenes.push((
                scene.frame.clone(),
                scene.registers,
                saved.textures.len() - 1,
            ));
        }
        let bytes = rmp_serde::to_vec(&saved).expect("the scenes encode");
        std::fs::write(path, bytes).expect("the scenes write");
    }

    fn load(path: &str) -> Self {
        let bytes = std::fs::read(path).expect("the scenes read");
        let saved: Saved = rmp_serde::from_slice(&bytes).expect("the scenes decode");
        Self::new(
            saved
                .scenes
                .into_iter()
                .map(|(frame, registers, at)| Scene {
                    frame,
                    registers,
                    textures: saved.textures[at].clone(),
                    set: at,
                })
                .collect(),
        )
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Saved {
    textures: Vec<Textures>,
    scenes: Vec<(Frame, Registers, usize)>,
}

fn keys(name: &str) -> Keys {
    name.split('+').fold(Keys::empty(), |all, part| {
        all | match part {
            "-" => Keys::empty(),
            "ST" => Keys::START,
            "UP" => Keys::UP,
            "DN" => Keys::DOWN,
            "LT" => Keys::LEFT,
            "RT" => Keys::RIGHT,
            other => Keys::from_name(other).unwrap_or_else(|| panic!("no key {other}")),
        }
    })
}

fn plan(text: &str) -> Vec<(Keys, u32)> {
    text.split(',')
        .filter(|step| !step.is_empty())
        .map(|step| {
            let (name, count) = step.split_once(':').expect("a step is KEY:FRAMES");
            (keys(name), count.parse().expect("a frame count"))
        })
        .collect()
}

fn boot(rom: &Arc<[u8]>, jit: bool) -> Nds {
    let mut nds = Nds::new(Arc::clone(rom), NdsFirmware::default()).expect("a DS ROM");
    nds.set_jit(jit);
    if let Ok(path) = std::env::var("SAVE_IN")
        && !path.is_empty()
    {
        let data = std::fs::read(path).expect("the save reads");
        let save = nds.battery_from_file(&data);
        nds.put_battery(save, 0);
    }
    nds
}

fn capture(
    rom: &Arc<[u8]>,
    jit: bool,
    lead: &[(Keys, u32)],
    walk: &[(Keys, u32)],
    count: usize,
) -> Run {
    let mut nds = boot(rom, jit);
    for &(keys, frames) in lead {
        nds.set_keys(keys);
        (0..frames).for_each(|_| nds.run_frame());
    }
    let mut steps = walk
        .iter()
        .cycle()
        .flat_map(|&(keys, frames)| std::iter::repeat_n(keys, frames as usize));
    let mut scenes: Vec<Scene> = Vec::with_capacity(count);
    for _ in 0..count {
        nds.set_keys(steps.next().expect("a walk step"));
        nds.run_frame();
        let scene = nds.three_d_scene();
        let set = match scenes.last() {
            Some(last)
                if last.textures.image() == scene.textures.image()
                    && last.textures.palette() == scene.textures.palette() =>
            {
                last.set
            }
            Some(last) => last.set + 1,
            None => 0,
        };
        scenes.push(Scene {
            frame: scene.frame.clone(),
            registers: scene.registers,
            textures: scene.textures.clone(),
            set,
        });
    }
    Run::new(scenes)
}

struct Live {
    textures: Textures,
    set: usize,
}

impl Live {
    fn take(&mut self, scene: &Scene) {
        if self.set != scene.set {
            self.textures = scene.textures.clone();
            self.set = scene.set;
        }
        if scene.registers.textured() {
            self.textures.prepare(&scene.frame);
        }
    }
}

fn render(renderer: &mut Renderer, scene: &Scene, textures: &Textures, lines: &mut [Line3d]) {
    renderer.start_frame(scene.registers);
    for (y, line) in lines.iter_mut().enumerate() {
        renderer.render_line(y, &scene.frame, textures, line);
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "the bench draws bands on plain threads to time the raster alone"
)]
fn render_bands(
    renderers: &mut [Renderer],
    scene: &Scene,
    textures: &Textures,
    lines: &mut [Line3d],
    carried: &mut Stencil,
    rows: usize,
) {
    let bands: Vec<_> = (0..HEIGHT)
        .step_by(rows)
        .map(|start| start..(start + rows).min(HEIGHT))
        .collect();
    let next = AtomicUsize::new(0);
    let (send, receive) = mpsc::channel();
    let start = &*carried;
    let ends: Vec<Option<Stencil>> = std::thread::scope(|scope| {
        let workers: Vec<_> = renderers
            .iter_mut()
            .map(|renderer| {
                let send = send.clone();
                let (bands, next) = (&bands, &next);
                scope.spawn(move || {
                    renderer.start_frame(scene.registers);
                    let mut end = None;
                    while let Some(band) = bands.get(next.fetch_add(1, Ordering::Relaxed)) {
                        let last = renderer.render_band(
                            band.clone(),
                            &scene.frame,
                            textures,
                            start,
                            |y, line| {
                                let _ = send.send((y, line.clone()));
                            },
                        );
                        end = end.or(last);
                    }
                    end
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().expect("a band thread ends"))
            .collect()
    });
    drop(send);
    for (y, line) in receive {
        lines[y] = line;
    }
    if let Some(end) = ends.into_iter().flatten().next() {
        *carried = end;
    }
}

fn pixels(lines: &[Line3d], hasher: &mut DefaultHasher) {
    for line in lines {
        line.color.hash(hasher);
        line.alpha.hash(hasher);
    }
}

fn ppm(path: &str, lines: &[Line3d]) {
    let mut out = format!("P6\n{WIDTH} {HEIGHT}\n255\n").into_bytes();
    for line in lines {
        for &color in &line.color {
            out.extend(
                [0, 6, 12].map(|shift| {
                    ((color >> shift & 0x3F) << 2 | (color >> shift & 0x3F) >> 4) as u8
                }),
            );
        }
    }
    std::fs::File::create(path)
        .and_then(|mut file| file.write_all(&out))
        .expect("the picture writes");
}

fn quantile(sorted: &[f64], q: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * q).round() as usize]
}

fn bench(name: &str, case: &str, run: &Run, reps: usize) {
    let rows = std::env::var("RASTER_BANDS").map_or(0, |text| text.parse().expect("rows per band"));
    let threads =
        std::env::var("RASTER_THREADS").map_or(1, |text| text.parse().expect("a thread count"));
    let case = if rows > 0 {
        format!("{case}-b{rows}t{threads}")
    } else {
        case.to_string()
    };
    let polygons: Vec<usize> = run
        .scenes
        .iter()
        .map(|scene| scene.frame.polygons.len())
        .collect();
    let mean_polygons = polygons.iter().sum::<usize>() as f64 / polygons.len() as f64;
    println!(
        "scene {name} {case} frames {} polygons mean {mean_polygons:.0} max {} distinct {}",
        run.scenes.len(),
        polygons.iter().max().copied().unwrap_or(0),
        run.digests
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
    );
    let mut renderer = Renderer::default();
    let mut renderers: Vec<Renderer> = (0..threads).map(|_| Renderer::default()).collect();
    let mut carried = Stencil::default();
    let mut lines = vec![Line3d::default(); HEIGHT];
    let mut best = vec![f64::MAX; run.scenes.len()];
    let mut hasher = DefaultHasher::new();
    let mut live = Live {
        textures: Textures::default(),
        set: usize::MAX,
    };
    for rep in 0..reps {
        for (at, scene) in run.scenes.iter().enumerate() {
            let start = Instant::now();
            live.take(scene);
            if rows > 0 {
                render_bands(
                    &mut renderers,
                    scene,
                    &live.textures,
                    &mut lines,
                    &mut carried,
                    rows,
                );
            } else {
                render(&mut renderer, scene, &live.textures, &mut lines);
            }
            best[at] = best[at].min(start.elapsed().as_secs_f64() * 1000.0);
            if rep == 0 {
                pixels(&lines, &mut hasher);
                if let Ok(dir) = std::env::var("RASTER_PPM")
                    && (at == 0 || at + 1 == run.scenes.len())
                {
                    ppm(&format!("{dir}/{name}-{case}-{at}.ppm"), &lines);
                }
            }
        }
    }
    let mean = best.iter().sum::<f64>() / best.len() as f64;
    best.sort_by(f64::total_cmp);
    println!(
        "raster {name} {case} mean {mean:.3} median {:.3} p90 {:.3} max {:.3} ms per frame, best of {reps}, pixels {:016x}",
        quantile(&best, 0.5),
        quantile(&best, 0.9),
        best[best.len() - 1],
        hasher.finish(),
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let reps = std::env::var("RASTER_REPS").map_or(5, |text| text.parse().expect("a rep count"));
    if let [_, mode, name, path] = args.as_slice()
        && mode == "replay"
    {
        bench(name, "replay", &Run::load(path), reps);
        return;
    }
    let [_, name, rom, lead, walk, cases] = args.as_slice() else {
        eprintln!("usage: ds_raster_bench <name> <rom> <lead plan> <walk plan> <jit-on,jit-off>");
        eprintln!("       ds_raster_bench replay <name> <scenes file>");
        std::process::exit(2);
    };
    let count =
        std::env::var("RASTER_FRAMES").map_or(120, |text| text.parse().expect("a frame count"));
    let rom: Arc<[u8]> = std::fs::read(rom).expect("the ROM reads").into();
    let (lead, walk) = (plan(lead), plan(walk));
    let mut runs: Vec<(&str, Run)> = Vec::new();
    for case in cases.split(',') {
        let jit = match case {
            "jit-on" => true,
            "jit-off" => false,
            other => panic!("no case {other}"),
        };
        let start = Instant::now();
        let run = capture(&rom, jit, &lead, &walk, count);
        println!(
            "captured {name} {case} in {:.1} s",
            start.elapsed().as_secs_f64()
        );
        if runs.is_empty()
            && let Ok(path) = std::env::var("RASTER_SAVE")
        {
            run.save(&path);
        }
        bench(name, case, &run, reps);
        runs.push((case, run));
    }
    if let [(_, first), (_, second)] = runs.as_slice() {
        let differ = first
            .digests
            .iter()
            .zip(&second.digests)
            .position(|(a, b)| a != b);
        match differ {
            None => println!("same {name} scenes in every case"),
            Some(at) => println!("same {name} scenes until frame {at}"),
        }
    }
}

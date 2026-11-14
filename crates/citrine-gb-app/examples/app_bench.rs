use citrine_gb::console::ConsoleKind;
use citrine_gb::rom::Rom;
use citrine_gb_app::Citrine;
use std::io::Read;
use std::path::Path;
use std::time::Instant;

const UPDATES: usize = 600;

struct Ui {
    app: Citrine,
    ctx: egui::Context,
    frame: eframe::Frame,
    input: egui::RawInput,
}

impl Ui {
    fn new() -> Self {
        let mut app = Citrine::default();
        app.ui.settings.check_updates = false;
        Self {
            app,
            ctx: egui::Context::default(),
            frame: eframe::Frame::_new_kittest(),
            input: egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                ..Default::default()
            },
        }
    }

    fn update(&mut self) -> usize {
        let output = self.ctx.run(self.input.clone(), |ctx| {
            eframe::App::update(&mut self.app, ctx, &mut self.frame);
        });
        self.ctx
            .tessellate(output.shapes, output.pixels_per_point)
            .len()
    }

    fn load(&mut self, path: &Path) {
        let (data, kind) = read_rom(path);
        self.app
            .emulator
            .load_game(&Rom::new(&data), kind, None)
            .expect("the rom loads");
    }
}

#[inline(never)]
fn measured<R>(work: impl FnOnce() -> R) -> R {
    std::hint::black_box(work())
}

fn timed<R>(work: impl FnOnce() -> R) -> (R, f64) {
    let start = Instant::now();
    let out = measured(work);
    (out, start.elapsed().as_secs_f64() * 1000.0)
}

fn read_rom(path: &Path) -> (Vec<u8>, ConsoleKind) {
    let bytes = std::fs::read(path).expect("the rom file");
    let name = path.to_string_lossy();
    let data = if name.ends_with(".br") {
        let mut out = Vec::new();
        brotli::Decompressor::new(bytes.as_slice(), 4096)
            .read_to_end(&mut out)
            .expect("a brotli rom");
        out
    } else {
        bytes
    };
    let kind = if name.contains(".gba") {
        ConsoleKind::Gba
    } else if name.contains(".gbc") {
        ConsoleKind::Gbc
    } else {
        ConsoleKind::Gb
    };
    (data, kind)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let case = args.next().unwrap_or_default();
    let rom = args.next();
    let rom = rom.as_deref().map(Path::new);
    let (ms, updates) = match case.as_str() {
        "start" => {
            let ((), ms) = timed(|| {
                let mut ui = Ui::new();
                if let Some(rom) = rom {
                    ui.load(rom);
                }
                ui.update();
            });
            (ms, 1)
        }
        "library" | "paused" => {
            let mut ui = Ui::new();
            if let Some(rom) = rom.filter(|_| case == "paused") {
                ui.load(rom);
                ui.app.emulator.running = false;
            }
            for _ in 0..30 {
                ui.update();
            }
            let (_, ms) = timed(|| (0..UPDATES).map(|_| ui.update()).sum::<usize>());
            (ms, UPDATES)
        }
        _ => {
            eprintln!("usage: app_bench start|library|paused [rom]");
            std::process::exit(2);
        }
    };
    println!(
        "{case} {:.4} ms per update, {updates} updates",
        ms / updates as f64
    );
}

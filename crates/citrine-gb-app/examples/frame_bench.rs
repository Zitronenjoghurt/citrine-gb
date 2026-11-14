use citrine_gb::console::ConsoleKind;
use citrine_gb::rom::Rom;
use citrine_gb_app::Citrine;
use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

const WARMUP: usize = 120;
const UPDATES: usize = 3600;
const PERIOD: Duration = Duration::from_nanos(16_666_667);

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: frame_bench <rom> [wait ms]");
    let wait = args
        .next()
        .map(|ms| ms.parse::<f64>().expect("a wait in ms"));
    let (data, kind) = read_rom(Path::new(&path));

    let mut app = Citrine::default();
    app.ui.settings.check_updates = false;
    app.emulator
        .load_game(&Rom::new(&data), kind, None)
        .expect("the rom loads");
    app.emulator.running = true;
    if let Some(ms) = wait {
        app.emulator
            .set_frame_wait(Duration::from_secs_f64(ms / 1000.0));
    }

    let ctx = egui::Context::default();
    let mut frame = eframe::Frame::_new_kittest();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 800.0),
        )),
        ..Default::default()
    };

    let mut times = Vec::with_capacity(UPDATES);
    let mut next = Instant::now();
    for update in 0..WARMUP + UPDATES {
        let start = Instant::now();
        let output = ctx.run(input.clone(), |ctx| {
            eframe::App::update(&mut app, ctx, &mut frame);
        });
        let _ = ctx.tessellate(output.shapes, output.pixels_per_point);
        let took = start.elapsed();
        if update >= WARMUP {
            times.push(took.as_secs_f64() * 1000.0);
        }
        next += PERIOD;
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        } else {
            next = now;
        }
    }

    let late = app.emulator.late_frames();
    let name = Path::new(&path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&path);
    println!(
        "| {name} | {} | {} |",
        summary(&mut times),
        wait.map_or("-".to_string(), |_| late.to_string())
    );
    eframe::App::on_exit(&mut app);
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
    } else if name.ends_with(".gbc") {
        ConsoleKind::Gbc
    } else {
        ConsoleKind::Gb
    };
    (data, kind)
}

fn summary(times: &mut [f64]) -> String {
    times.sort_by(f64::total_cmp);
    let at = |q: f64| times[((times.len() - 1) as f64 * q).round() as usize];
    let mean = times.iter().sum::<f64>() / times.len() as f64;
    format!(
        "{:.3} | {:.3} | {:.3} | {:.3} | {}",
        at(0.5),
        at(0.99),
        times[times.len() - 1],
        mean,
        times.len()
    )
}

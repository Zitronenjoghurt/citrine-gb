use crate::app::Citrine;
use crate::app::tabs::{Tab, Tool};
use crate::app::widgets::video::VideoTab;
use base64::Engine;
use citrine_gb::debug::Debug;
use citrine_gb::debug::code::Address;
use citrine_gb::debug::step::Step;
use std::cell::{Cell, RefCell};
use wasm_bindgen::prelude::wasm_bindgen;

const DEBUG_FRAMES: u32 = 60;
const BENCH_UPDATES: usize = 4000;

#[derive(Clone, Copy)]
enum Target {
    Video(VideoTab),
    Debug(Tab),
}

fn read(name: &str) -> Option<String> {
    let storage = web_sys::window()?.local_storage().ok()??;
    storage.get_item(&format!("citrine/dev/{name}")).ok()?
}

pub fn open(app: &mut Citrine) {
    let Some(rom) = read("rom") else {
        return;
    };
    let Ok(data) = base64::engine::general_purpose::STANDARD.decode(rom.trim()) else {
        return;
    };
    app.events.load_rom_data(data);
    app.resume_game();
    let view = read("video").unwrap_or_default();
    let target = VideoTab::ALL
        .into_iter()
        .find(|tab| tab.label().eq_ignore_ascii_case(view.trim()))
        .map(Target::Video)
        .or_else(|| debug_tab().map(Target::Debug));
    let fallback = match target {
        Some(Target::Debug(_)) => DEBUG_FRAMES,
        _ => 0,
    };
    let frames = read("frames")
        .and_then(|frames| frames.trim().parse().ok())
        .unwrap_or(fallback);
    let due = web_time::Instant::now() + std::time::Duration::from_secs(2);
    PENDING.set(target.map(|target| (target, due, frames)));
}

fn debug_tab() -> Option<Tab> {
    match read("tab")?.trim() {
        "debugger" => Some(Tab::Disassembly),
        "memory" => Some(Tab::Tool(Tool::Memory)),
        "registers" => Some(Tab::Registers),
        "rom-info" => Some(Tab::RomInfo),
        "sound" => Some(Tab::Apu),
        "threads" => Some(Tab::Threads),
        _ => None,
    }
}

thread_local! {
    static PENDING: Cell<Option<(Target, web_time::Instant, u32)>> = const { Cell::new(None) };
    static BENCH: RefCell<Bench> = RefCell::default();
}

#[derive(Default, serde::Serialize)]
struct Bench {
    #[serde(skip)]
    started: Option<web_time::Instant>,
    updates: Vec<f32>,
    late: u64,
    frames: u64,
    worker: bool,
    underruns: u64,
    underrun_frames: u64,
    overruns: u64,
    playing: bool,
}

pub fn tock(app: &Citrine) {
    BENCH.with_borrow_mut(|bench| {
        if let Some(started) = bench.started.take() {
            if bench.updates.len() >= BENCH_UPDATES {
                bench.updates.remove(0);
            }
            bench.updates.push(started.elapsed().as_secs_f32() * 1000.0);
        }
        #[cfg(threaded)]
        {
            bench.late = app.emulator.late_frames();
        }
        bench.frames = app.emulator.total_frames as u64;
        bench.worker = app.emulator.has_worker();
        if let Some(audio) = &app.audio {
            let stats = audio.stats();
            bench.underruns = stats.underruns();
            bench.underrun_frames = stats.underrun_frames();
            bench.overruns = stats.overruns();
            bench.playing = stats.is_playing();
        }
    });
}

#[wasm_bindgen]
pub fn citrine_dev_bench(reset: bool) -> String {
    BENCH.with_borrow_mut(|bench| {
        let text = serde_json::to_string(&*bench).unwrap_or_default();
        if reset {
            bench.updates.clear();
        }
        text
    })
}

pub fn tick(app: &mut Citrine, ctx: &egui::Context) {
    BENCH.with_borrow_mut(|bench| bench.started = Some(web_time::Instant::now()));
    let Some((target, due, frames)) = PENDING.get() else {
        return;
    };
    if web_time::Instant::now() < due {
        return;
    }
    PENDING.set(None);
    if frames > 0 {
        app.emulator.soft_reset(ctx);
    }
    if let Target::Debug(tab) = target {
        let machine = app.emulator.debug_mut();
        machine.set_code_analysis(true);
        machine.record_waves(tab == Tab::Apu);
    }
    for _ in 0..frames {
        app.emulator.step(Step::Frame);
    }
    match target {
        Target::Video(tab) => {
            app.ui.video.tab = tab;
            app.events.open_tab(Tab::Tool(Tool::Video));
        }
        Target::Debug(tab) => {
            app.emulator.running = tab == Tab::Threads;
            open_debug_views(app);
            app.events.open_tab(tab);
        }
    }
}

fn open_debug_views(app: &mut Citrine) {
    if read("cpu").as_deref() == Some("open") {
        app.ui.registers.docked_cpu = true;
    }
    for group in read("groups").unwrap_or_default().split(',') {
        if !group.is_empty() {
            *app.ui.registers.open_mut(group) = true;
        }
    }
    if let Some(name) = read("block") {
        let mut blocks = Vec::new();
        app.emulator.debug().memory_blocks(&mut blocks);
        if let Some(block) = blocks.iter().find(|block| block.name == name) {
            let at = Address::at(block.id, block.first_bank, 0);
            app.ui.memory.show_at(block, at, 1);
        }
    }
}

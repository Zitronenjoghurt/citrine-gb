use crate::app::Citrine;
use crate::app::tabs::{Tab, Tool};
use crate::app::widgets::video::VideoTab;
use base64::Engine;
use citrine_gb::debug::Debug;
use citrine_gb::debug::code::Address;
use citrine_gb::debug::step::Step;
use std::cell::Cell;

const DEBUG_FRAMES: u32 = 60;

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
        _ => None,
    }
}

thread_local! {
    static PENDING: Cell<Option<(Target, web_time::Instant, u32)>> = const { Cell::new(None) };
}

pub fn tick(app: &mut Citrine, ctx: &egui::Context) {
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
            app.emulator.running = false;
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

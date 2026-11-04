use super::*;
use citrine_gb::console::Console;
use citrine_gb::debug::Debug;
use citrine_gb::debug::code::{Address, BlockId};
use citrine_gb::debug::step::Step;

const END_LOOP: u32 = 0x0C;

fn program() -> Vec<u8> {
    [
        0xE3A0_C000_u32,
        0xEB00_0001,
        0xE3A0_1002,
        0xEAFF_FFFE,
        0xE3A0_2003,
        0xE12F_FF1E,
    ]
    .iter()
    .flat_map(|word| word.to_le_bytes())
    .collect()
}

fn cart(offset: u32) -> Address {
    Address::gba(BlockId::GamePak, offset)
}

const PRINT_HI: [u32; 16] = [
    0xE3A0_0404,
    0xE280_08FF,
    0xE280_0CF6,
    0xE3A0_1CC0,
    0xE281_10DE,
    0xE280_2E18,
    0xE1C2_10B0,
    0xE3A0_3048,
    0xE5C0_3000,
    0xE3A0_3069,
    0xE5C0_3001,
    0xE280_2C01,
    0xE3A0_1C01,
    0xE281_1003,
    0xE1C2_10B0,
    0xEAFF_FFFE,
];

fn words(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

pub(super) fn gba_app(name: &str) -> Citrine {
    gba_app_with(name, &program())
}

fn gba_app_with(name: &str, rom: &[u8]) -> Citrine {
    let mut app = phone_app(name);
    app.ui.settings.layout = LayoutMode::Desktop;
    app.emulator.load_gba(rom).expect("the program loads");
    app.emulator.debug_mut().set_code_analysis(true);
    app
}

fn drawn(output: &egui::FullOutput, wanted: &str) -> Option<(egui::Rect, egui::Rect)> {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.text().trim() == wanted => {
                Some((text.visual_bounding_rect(), clipped.clip_rect))
            }
            _ => None,
        })
}

fn in_view(output: &egui::FullOutput, wanted: &str) -> bool {
    drawn(output, wanted).is_some_and(|(rect, clip)| clip.contains_rect(rect))
}

fn pc(app: &Citrine) -> Option<Address> {
    app.emulator.debug().pc()
}

#[test]
fn the_debugger_shows_arm_code_and_every_cpu_bank_of_a_gba_game() {
    let ctx = Context::default();
    let mut app = gba_app("gba-debugger-code");
    app.emulator.debug_mut().run_frame();
    app.ui.registers.docked_cpu = true;
    app.open_tab(Tab::Disassembly);

    let output = settle(&mut app, &ctx, egui::vec2(1280.0, 1440.0), 1.0);

    for wanted in ["08000004", "BL", "R12", "CPSR", "Mode System", "FIQ", "IRQ"] {
        assert!(in_view(&output, wanted), "{wanted} is in view");
    }
    assert!(drawn(&output, "Log").is_none(), "no log before a print");
    let texts: Vec<String> = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect();
    assert!(texts.iter().any(|text| text.ends_with(" cycles, ")));
    assert!(!texts.iter().any(|text| text.contains("M-cycle")));
}

#[test]
fn step_over_runs_a_bl_and_step_out_returns_from_it() {
    let mut app = gba_app("gba-debugger-steps");

    app.emulator.step(Step::Instruction);
    assert_eq!(pc(&app), Some(cart(0x04)));
    app.emulator.step(Step::Over);
    assert_eq!(pc(&app), Some(cart(0x08)));

    let mut app = gba_app("gba-debugger-out");
    app.emulator.step(Step::Instruction);
    app.emulator.step(Step::Instruction);
    assert_eq!(pc(&app), Some(cart(0x10)));
    app.emulator.step(Step::Out);
    assert_eq!(pc(&app), Some(cart(0x08)));
}

#[test]
fn a_breakpoint_on_the_end_loop_stops_with_r12_in_view() {
    let ctx = Context::default();
    let mut app = gba_app("gba-debugger-end");
    app.emulator
        .debug_mut()
        .breakpoints()
        .insert(cart(END_LOOP));
    app.emulator.debug_mut().run_frame();
    assert!(app.emulator.debug_mut().take_breakpoint_hit());
    assert_eq!(pc(&app), Some(cart(END_LOOP)));
    app.open_tab(Tab::Disassembly);

    let output = settle(&mut app, &ctx, egui::vec2(1280.0, 800.0), 1.0);

    assert!(in_view(&output, "R12"));
    assert!(in_view(&output, "0800000C"));
}

#[test]
fn lines_a_game_prints_show_in_the_debugger_log() {
    let ctx = Context::default();
    let mut app = gba_app_with("gba-debugger-log", &words(&PRINT_HI));
    app.emulator.debug_mut().run_frame();
    app.open_tab(Tab::Disassembly);

    for height in [800.0, 1440.0] {
        let output = settle(&mut app, &ctx, egui::vec2(1280.0, height), 1.0);

        for wanted in ["Log", "Info", "Hi"] {
            assert!(in_view(&output, wanted), "{wanted} is in view at {height}");
        }
    }
}

#[test]
fn four_printed_lines_fit_under_the_code_in_a_short_debugger() {
    use citrine_gb::debug::print::{PrintLevel, PrintLine};
    let ctx = Context::default();
    let mut app = gba_app_with("gba-debugger-log-lines", &words(&PRINT_HI));
    let log = app
        .emulator
        .debug_mut()
        .print_log()
        .expect("a GBA has a log");
    let texts = ["first line", "second line", "third line", "fourth line"];
    for text in texts {
        log.push(PrintLine {
            level: PrintLevel::Warn,
            text: text.into(),
        });
    }
    app.open_tab(Tab::Disassembly);

    for (size, ratio) in [
        (egui::vec2(1280.0, 800.0), 2.0),
        (egui::vec2(1280.0, 560.0), 1.0),
    ] {
        let output = settle(&mut app, &ctx, size, ratio);

        let shown: Vec<String> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text)
                    if clipped.clip_rect.contains_rect(text.visual_bounding_rect()) =>
                {
                    Some(text.galley.text().to_owned())
                }
                _ => None,
            })
            .collect();
        let newest = if size.y < 600.0 {
            &texts[1..]
        } else {
            &texts[..]
        };
        for text in newest {
            assert!(
                shown.iter().any(|line| line.ends_with(text)),
                "{text} is in view at {size:?}"
            );
        }
        let (clear, clip) = drawn(&output, "Clear").expect("Clear is drawn");
        assert!(
            clear.right() + 2.0 <= clip.right(),
            "Clear and its button edge are in view at {size:?}"
        );
    }
}

#[test]
fn the_sound_tool_lists_six_channels_and_mutes_them_from_its_own_setting() {
    use citrine_gb::machine::Machine;
    let ctx = Context::default();
    let mut app = gba_app("gba-sound-tool");
    app.open_tab(Tab::Apu);
    app.ui.settings.gba_channels[5] = false;

    let output = settle(&mut app, &ctx, egui::vec2(1280.0, 1440.0), 1.0);

    for name in ["Pulse 1", "Noise", "Direct Sound A", "Direct Sound B"] {
        assert!(in_view(&output, name), "{name} is in view");
    }
    let Machine::Gba(gba) = app.emulator.debug() else {
        panic!("a GBA is loaded");
    };
    let sound = gba.bus().sound();
    assert!(!sound.channel_enabled(5));
    assert!((0..5).all(|channel| sound.channel_enabled(channel)));
    assert_eq!(app.ui.settings.channels, [true; 4]);
    app.emulator.debug_mut().record_waves(true);
    app.emulator.debug_mut().run_frame();
    assert_eq!(app.emulator.debug().waves().len(), 6);
}

#[test]
fn register_fields_wrap_as_tight_text_lines_on_a_phone() {
    let ctx = Context::default();
    let mut app = gba_app("gba-registers-phone");
    app.ui.settings.layout = LayoutMode::Mobile;
    *app.ui.registers.open_mut("Display") = true;
    app.mobile.open(Tab::Registers);

    let output = settle(&mut app, &ctx, egui::vec2(412.0, 915.0), 1.0);

    assert!(in_view(&output, "DISPCNT"), "DISPCNT is in view");
    let mut tops: Vec<f32> = ["OAM in HBlank", "Forced blank", "BG3", "OBJ window"]
        .iter()
        .map(|name| {
            drawn(&output, name)
                .unwrap_or_else(|| panic!("{name} is drawn"))
                .0
                .top()
        })
        .collect();
    tops.sort_by(f32::total_cmp);
    tops.dedup_by(|a, b| (*a - *b).abs() < 1.0);
    assert!(tops.len() > 1, "the fields wrap at 412");
    let line = ctx.style().text_styles[&egui::TextStyle::Small].size * 2.0;
    for pair in tops.windows(2) {
        assert!(
            pair[1] - pair[0] < line,
            "field lines sit {} apart",
            pair[1] - pair[0]
        );
    }
}

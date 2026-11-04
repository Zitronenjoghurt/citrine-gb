use crate::debug::step::{Step, Stop};
use crate::gb::{GameBoy, GbModel};
use crate::rom::Rom;

const TONE: [u8; 22] = [
    0x3E, 0xF0, 0xE0, 0x17, 0x3E, 0x80, 0xE0, 0x16, 0x3E, 0x00, 0xE0, 0x18, 0x3E, 0x87, 0xE0, 0x19,
    0x3E, 0xFF, 0xE0, 0x25, 0x18, 0xFE,
];

fn rom_with(code: &[u8]) -> Rom {
    let mut data = vec![0u8; 0x8000];
    data[0x0100..0x0104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]);
    data[0x0150..0x0150 + code.len()].copy_from_slice(code);
    Rom::new(&data)
}

fn console(code: &[u8]) -> GameBoy {
    let mut gb = GameBoy::new_empty(GbModel::Dmg);
    gb.load_rom(&rom_with(code)).expect("load");
    gb
}

#[test]
fn stepping_counts_on_from_the_frame_position() {
    let mut gb = console(&[0x18, 0xFE]);
    gb.run_frame();
    gb.cycle_counter = 10_000;

    gb.run_for(1_000);

    let counter = gb.cycle_counter;
    assert!((11_000..11_100).contains(&counter), "counter {counter}");
}

#[test]
fn stepping_past_the_frame_end_wraps_like_a_frame() {
    let mut gb = console(&[0x18, 0xFE]);
    gb.run_frame();
    let frame = gb.frame_cycles();
    gb.cycle_counter = frame - 500;

    gb.run_for(1_000);

    let counter = gb.cycle_counter;
    assert!((500..600).contains(&counter), "counter {counter}");
}

#[test]
fn stepping_stops_at_a_breakpoint() {
    let mut gb = console(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0xFE]);
    let target = gb.code_address(0x0154).expect("rom");
    gb.debugger.breakpoints.insert(target);

    let stop = gb.run_for(100_000);

    assert_eq!(stop, Stop::Breakpoint(target));
    assert_eq!(gb.cpu.pc.wrapping_sub(1), 0x0154);
    assert!(!gb.debugger.hit_breakpoint);
}

#[test]
fn stepping_hands_its_sound_out_before_the_next_frame() {
    let mut gb = console(&TONE);
    gb.run_frame();
    gb.run_frame();
    gb.apu.audio_buffer.clear();

    gb.run_for(10_000);
    let stepped = gb.apu.audio_buffer.len();
    gb.apu.audio_buffer.clear();
    gb.run_frame();
    let frame = gb.apu.audio_buffer.len();

    let per_frame =
        (gb.apu.output_sample_rate as u64 * 2 * 4 * gb.frame_cycles() as u64 / 4_194_304) as usize;
    assert!(stepped > 600, "stepped {stepped}");
    assert!(
        frame <= per_frame + 8,
        "frame {frame}, one frame is {per_frame}"
    );
}

#[test]
fn running_for_a_frame_of_cycles_matches_a_frame() {
    let rom = Rom::new(include_bytes!("../../../citrine-gb-app/homebrew/tobu.gb"));
    let [mut framed, mut stepped] = [0, 1].map(|_| {
        let mut gb = GameBoy::new_empty(GbModel::Dmg);
        gb.load_rom(&rom).expect("load");
        for _ in 0..120 {
            gb.run_frame();
        }
        gb.apu.audio_buffer.clear();
        gb
    });

    framed.run_frame();
    let frame = framed.frame_cycles();
    assert_eq!(stepped.run_for(frame), Stop::Finished);

    assert!(stepped.frame().as_slice() == framed.frame().as_slice());
    assert_eq!(stepped.cycle_counter, framed.cycle_counter);
    assert_eq!(
        stepped.apu.audio_buffer.len(),
        framed.apu.audio_buffer.len()
    );
}

const CALLS: [(u16, &[u8]); 5] = [
    (
        0x0150,
        &[
            0x31, 0xFE, 0xDF, 0xCD, 0x60, 0x01, 0xCD, 0x70, 0x01, 0x18, 0xFE,
        ],
    ),
    (0x0160, &[0x00, 0xCD, 0x68, 0x01, 0x00, 0xC9]),
    (0x0168, &[0x00, 0xC9]),
    (
        0x0170,
        &[0x01, 0x00, 0x40, 0x0B, 0x78, 0xB1, 0x20, 0xFB, 0xC9],
    ),
    (0x0180, &[]),
];

fn calls() -> GameBoy {
    let mut data = vec![0u8; 0x8000];
    data[0x0100..0x0104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]);
    for (at, code) in CALLS {
        let at = at as usize;
        data[at..at + code.len()].copy_from_slice(code);
    }
    let mut gb = GameBoy::new_empty(GbModel::Dmg);
    gb.load_rom(&Rom::new(&data)).expect("load");
    gb
}

fn line(gb: &GameBoy) -> u16 {
    gb.cpu.pc.wrapping_sub(1)
}

fn walk_to(gb: &mut GameBoy, at: u16) {
    for _ in 0..50 {
        if line(gb) == at {
            return;
        }
        assert_eq!(gb.run_step(Step::Instruction), Stop::Finished);
    }
    panic!("never reached {at:04X}, at {:04X}", line(gb));
}

fn finish(gb: &mut GameBoy, step: Step) -> (Stop, usize) {
    let mut stop = gb.run_step(step);
    let mut calls = 1;
    while stop == Stop::Unfinished && calls < 40 {
        stop = gb.resume_step();
        calls += 1;
    }
    (stop, calls)
}

#[test]
fn an_instruction_step_enters_a_call() {
    let mut gb = calls();
    walk_to(&mut gb, 0x0153);

    assert_eq!(gb.run_step(Step::Instruction), Stop::Finished);

    assert_eq!(line(&gb), 0x0160);
}

#[test]
fn stepping_over_a_call_lands_on_the_next_line_with_the_same_stack() {
    let mut gb = calls();
    walk_to(&mut gb, 0x0153);
    let sp = gb.cpu.sp;

    assert_eq!(gb.run_step(Step::Over), Stop::Finished);

    assert_eq!(line(&gb), 0x0156);
    assert_eq!(gb.cpu.sp, sp);
    assert!(!gb.step_pending());
}

#[test]
fn stepping_over_a_plain_line_is_one_instruction() {
    let mut gb = calls();
    walk_to(&mut gb, 0x0150);

    assert_eq!(gb.run_step(Step::Over), Stop::Finished);

    assert_eq!(line(&gb), 0x0153);
}

#[test]
fn stepping_out_returns_to_the_caller_one_frame_at_a_time() {
    let mut gb = calls();
    walk_to(&mut gb, 0x0153);
    let outer = gb.cpu.sp;
    walk_to(&mut gb, 0x0161);
    let inner = gb.cpu.sp;
    walk_to(&mut gb, 0x0168);

    assert_eq!(gb.run_step(Step::Out), Stop::Finished);
    assert_eq!((line(&gb), gb.cpu.sp), (0x0164, inner));

    assert_eq!(gb.run_step(Step::Out), Stop::Finished);
    assert_eq!((line(&gb), gb.cpu.sp), (0x0156, outer));
}

#[test]
fn a_slow_call_keeps_its_step_across_calls_until_it_returns() {
    let mut gb = calls();
    walk_to(&mut gb, 0x0156);
    let sp = gb.cpu.sp;

    let (stop, calls) = finish(&mut gb, Step::Over);

    assert_eq!(stop, Stop::Finished);
    assert!(calls > 1, "calls {calls}");
    assert_eq!((line(&gb), gb.cpu.sp), (0x0159, sp));
}

#[test]
fn a_cancelled_step_leaves_nothing_pending() {
    let mut gb = calls();
    walk_to(&mut gb, 0x0156);

    assert_eq!(gb.run_step(Step::Over), Stop::Unfinished);
    assert!(gb.step_pending());
    gb.cancel_step();

    assert!(!gb.step_pending());
    assert_eq!(gb.resume_step(), Stop::Finished);
}

#[test]
fn stepping_over_a_call_stops_at_a_breakpoint_inside_it() {
    let mut gb = calls();
    let target = gb.code_address(0x0168).expect("rom");
    gb.debugger.breakpoints.insert(target);
    walk_to(&mut gb, 0x0153);

    assert_eq!(gb.run_step(Step::Over), Stop::Breakpoint(target));

    assert_eq!(line(&gb), 0x0168);
    assert!(!gb.step_pending());
}

#[test]
fn a_cycle_step_runs_that_many_cycles() {
    let mut gb = calls();
    walk_to(&mut gb, 0x0156);
    finish(&mut gb, Step::Over);
    let before = gb.debugger.total_cycles;

    let (stop, _) = finish(&mut gb, Step::Cycles(100_000));

    assert_eq!(stop, Stop::Finished);
    let ran = gb.debugger.total_cycles - before;
    assert!((100_000..100_004).contains(&ran), "ran {ran}");
}

fn tobu() -> GameBoy {
    let rom = Rom::new(include_bytes!("../../../citrine-gb-app/homebrew/tobu.gb"));
    let mut gb = GameBoy::new_empty(GbModel::Dmg);
    gb.load_rom(&rom).expect("load");
    for _ in 0..120 {
        gb.run_frame();
    }
    gb.apu.audio_buffer.clear();
    gb
}

#[test]
fn a_frame_step_matches_a_frame() {
    let [mut framed, mut stepped] = [0, 1].map(|_| tobu());

    framed.run_frame();
    assert_eq!(stepped.run_step(Step::Frame), Stop::Finished);

    assert!(stepped.frame().as_slice() == framed.frame().as_slice());
    assert_eq!(stepped.cycle_counter, framed.cycle_counter);
    assert_eq!(stepped.cpu.pc, framed.cpu.pc);
    assert_eq!(
        stepped.apu.audio_buffer.len(),
        framed.apu.audio_buffer.len()
    );
}

#[test]
fn a_line_step_moves_to_the_next_line() {
    let mut gb = tobu();
    for _ in 0..200 {
        let line = gb.ppu.line();

        assert_eq!(gb.run_step(Step::Line), Stop::Finished);

        assert_eq!(gb.ppu.line(), (line + 1) % 154, "from line {line}");
    }
}

#[test]
fn a_line_step_with_the_screen_off_takes_one_line_of_cycles() {
    let mut gb = calls();
    walk_to(&mut gb, 0x0156);
    finish(&mut gb, Step::Over);
    gb.ppu.lcdc.lcd_enabled = false;
    let before = gb.debugger.total_cycles;

    assert_eq!(gb.run_step(Step::Line), Stop::Finished);

    let ran = gb.debugger.total_cycles - before;
    let line = gb.frame_cycles() as u128 / 154;
    assert!((line..line + 4).contains(&ran), "ran {ran}");
}

use crate::disassembly::DisassemblySource;
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

    gb.step_cycles(1_000);

    let counter = gb.cycle_counter;
    assert!((11_000..11_100).contains(&counter), "counter {counter}");
}

#[test]
fn stepping_past_the_frame_end_wraps_like_a_frame() {
    let mut gb = console(&[0x18, 0xFE]);
    gb.run_frame();
    let frame = gb.frame_cycles();
    gb.cycle_counter = frame - 500;

    gb.step_cycles(1_000);

    let counter = gb.cycle_counter;
    assert!((500..600).contains(&counter), "counter {counter}");
}

#[test]
fn stepping_stops_at_a_breakpoint() {
    let mut gb = console(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0xFE]);
    let target = gb.cartridge.probe_rom_location(0x0154);
    gb.debugger.breakpoints.insert(target);

    let stopped = gb.step_cycles(100_000);

    assert!(stopped);
    assert_eq!(gb.cpu.pc.wrapping_sub(1), 0x0154);
    assert!(!gb.debugger.hit_breakpoint);
}

#[test]
fn stepping_hands_its_sound_out_before_the_next_frame() {
    let mut gb = console(&TONE);
    gb.run_frame();
    gb.run_frame();
    gb.apu.audio_buffer.clear();

    gb.step_cycles(10_000);
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

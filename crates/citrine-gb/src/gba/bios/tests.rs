use super::*;
use crate::console::ConsoleKind;
use crate::gba::machine::{CYCLES_PER_FRAME, Gba};
use crate::gba::power::Power;
use crate::machine::{Firmware, Machine};
use crate::rom::Rom;
use std::f64::consts::FRAC_PI_2;

const LOOP: u32 = 0xEAFF_FFFE;
const LOAD_ARGUMENT: u32 = 0xE59F_0010;
const RETURNED: u32 = 0x0800_0014;
const EWRAM: u32 = 0x0200_0000;
const IWRAM: u32 = 0x0300_0000;
const VRAM: u32 = 0x0600_0000;
const VCOUNT: u32 = 0x0400_0006;
const IRQ_CHECK: u32 = 0x0300_7FF8;
const IRQ_HANDLER: u32 = 0x0300_7FFC;
const BUILT_IN_SHA256: &str = "bdc8a51380cc18a41ecb1f7140f5a8dcc23840b9ae93cd233e92c8aa2305cb53";

fn rom(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

fn swi(number: u32, args: [u32; 4]) -> Gba {
    let mut words: Vec<u32> = (0..4).map(|r| LOAD_ARGUMENT | r << 12).collect();
    words.extend([0xEF00_0000 | number << 16, LOOP]);
    words.extend(args);
    Gba::new(&rom(&words))
}

fn run(gba: &mut Gba) {
    let limit = gba.cycles() + 8 * CYCLES_PER_FRAME;
    while !(gba.cpu().reg(15) - 8 == RETURNED && gba.power() == Power::Running) {
        assert!(gba.cycles() < limit, "the call did not return");
        assert_eq!(gba.fault(), None);
        gba.step();
    }
}

fn call(number: u32, args: [u32; 4]) -> Gba {
    let mut gba = swi(number, args);
    run(&mut gba);
    gba
}

fn call_with(number: u32, args: [u32; 4], setup: impl FnOnce(&mut Gba)) -> Gba {
    let mut gba = swi(number, args);
    setup(&mut gba);
    run(&mut gba);
    gba
}

fn write_words(gba: &mut Gba, addr: u32, words: &[u32]) {
    for (i, word) in words.iter().enumerate() {
        gba.bus_mut().write32(addr + 4 * i as u32, *word);
    }
}

fn write_bytes(gba: &mut Gba, addr: u32, bytes: &[u8]) {
    for (i, byte) in bytes.iter().enumerate() {
        gba.bus_mut().write8(addr + i as u32, *byte);
    }
}

fn read_bytes(gba: &Gba, addr: u32, len: u32) -> Vec<u8> {
    (0..len).map(|i| gba.bus().read8(addr + i)).collect()
}

fn reg(gba: &Gba, index: usize) -> u32 {
    gba.cpu().reg(index)
}

fn half(gba: &Gba, addr: u32) -> i16 {
    gba.bus().read16(addr) as i16
}

fn angle(turns_of_half_pi: f64) -> i32 {
    (turns_of_half_pi * 16384.0).round() as i32
}

#[test]
fn the_built_in_image_is_the_pinned_cult_of_gba_build() {
    let digest: [u8; 32] = <sha2::Sha256 as sha2::Digest>::digest(CULT_OF_GBA).into();
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(hex, BUILT_IN_SHA256);
}

#[test]
fn the_built_in_image_keeps_the_official_handler_addresses() {
    let word = |addr: usize| u32::from_le_bytes(CULT_OF_GBA[addr..addr + 4].try_into().unwrap());
    assert_eq!(word(0xE4), 0xE129_F000);
    assert_eq!(word(0x128), 0xE92D_500F);
    assert_eq!(word(0x13C), 0xE25E_F004);
    assert_eq!(word(0x144), 0xE55E_C002);
    assert_eq!(word(0x188), 0xE1B0_F00E);
    assert_eq!(word(0x190), 0xE3A0_2004);
}

#[test]
fn a_user_bios_must_be_16_kb() {
    assert_eq!(Bios::user(&[0; 0x100]), Err(BiosError::Size(0x100)));
    assert_eq!(Bios::user(&[0; 0x4001]), Err(BiosError::Size(0x4001)));
    let bios = Bios::user(&[0xAB; 0x4000]).expect("16 KB");
    assert_eq!(bios.kind(), BiosKind::User);
    assert_eq!(bios.image()[0x3FFF], 0xAB);
}

#[test]
fn the_machine_reports_which_bios_runs() {
    assert_eq!(Gba::new(&rom(&[LOOP])).bios(), BiosKind::BuiltIn);
    let mut image = CULT_OF_GBA.to_vec();
    image[0x3FFC..].copy_from_slice(&0x1234_5678u32.to_le_bytes());
    let user = Bios::user(&image).expect("16 KB");
    let mut gba = Gba::with_bios(&rom(&[LOOP]), user, Boot::Skip);
    assert_eq!(gba.bios(), BiosKind::User);
    assert_eq!(gba.bus().read32(0x3FFC), 0x1234_5678);
    gba.soft_reset();
    assert_eq!(gba.bios(), BiosKind::User);
    assert_eq!(gba.bus().read32(0x3FFC), 0x1234_5678);
}

#[test]
fn the_firmware_slot_carries_a_user_bios_into_the_machine() {
    let firmware = Firmware {
        gba_bios: Some(Bios::user(CULT_OF_GBA).expect("16 KB")),
        gba_boot: Boot::Intro,
        ..Firmware::default()
    };
    let machine =
        Machine::load(&Rom::new(&rom(&[LOOP])), ConsoleKind::Gba, &firmware).expect("load");
    let Machine::Gba(gba) = machine else {
        panic!("not a gba");
    };
    assert_eq!(gba.bios(), BiosKind::User);
    assert_eq!(gba.boot(), Boot::Intro);
}

#[test]
fn skipping_the_intro_leaves_the_registers_the_bios_leaves() {
    let gba = Gba::new(&rom(&[LOOP]));
    assert_eq!(gba.boot(), Boot::Skip);
    assert_eq!(gba.bus().read16(0x0400_0000), 0x0080);
    assert_eq!(gba.bus().ppu().affine_matrix(0)[0], 0x0100);
    assert_eq!(gba.bus().ppu().affine_matrix(1)[3], 0x0100);
    assert_eq!(gba.bus().read16(0x0400_0134), 0x8000);
    assert_eq!(gba.bus().read8(0x0400_0300), 1);
}

fn header_boot_byte(value: u8) -> Vec<u8> {
    let mut image = rom(&[LOOP; 0x30]);
    image[0xB4] = value;
    image
}

#[test]
fn skipping_the_intro_starts_the_cartridge_where_the_bios_hands_over() {
    let gba = Gba::new(&rom(&[LOOP]));
    assert_eq!(gba.cycles(), 76_001_689);
    assert_eq!(gba.cpu().reg(14), 0x0800_0000);
    assert_eq!(gba.bus().read16(VCOUNT), 126);
    assert_eq!(gba.bus().read16(0x0400_0004), 0);
    assert_eq!(gba.bus().read16(0x0400_0202), 0);
}

#[test]
fn skipping_the_intro_runs_the_first_frame_to_the_real_frame_end() {
    let mut gba = Gba::new(&rom(&[LOOP]));
    gba.run_frame();
    assert!(gba.cycles() >= 271 * CYCLES_PER_FRAME);
    assert!(gba.cycles() < 271 * CYCLES_PER_FRAME + 32);
}

#[test]
fn skipping_the_intro_leaves_timer_0_where_the_bios_stopped_it() {
    let gba = Gba::new(&header_boot_byte(0x00));
    assert_eq!(gba.bus().read16(0x0400_0100), 0xFF8A);
    assert_eq!(gba.bus().read16(0x0400_0102), 0);
    assert_eq!(gba.bus().timers().reload(0), 0);
}

#[test]
fn the_header_byte_b4_with_bit_7_set_starts_timer_0_a_cycle_earlier() {
    let gba = Gba::new(&header_boot_byte(0x80));
    assert_eq!(gba.bus().read16(0x0400_0100), 0xFF8B);
}

#[test]
fn skipping_the_intro_leaves_the_prescaler_where_the_intro_leaves_it() {
    let program = rom(&[LOOP]);
    let mut intro = through_the_intro(&program);
    let mut skipped = Gba::new(&program);
    assert_eq!(intro.cycles(), skipped.cycles());
    for gba in [&mut intro, &mut skipped] {
        gba.bus_mut().write16(0x0400_0106, 0x0081);
    }
    for _ in 0..200 {
        intro.step();
        skipped.step();
        assert_eq!(intro.cycles(), skipped.cycles());
        assert_eq!(
            intro.bus().timers().counter(1),
            skipped.bus().timers().counter(1)
        );
    }
}

#[test]
fn the_intro_runs_from_the_reset_vector_into_the_cartridge() {
    let program = rom(&[LOOP]);
    let mut gba = Gba::with_bios(&program, Bios::built_in(), Boot::Intro);
    assert_eq!(gba.cpu().reg(15) - 8, 0);
    assert_eq!(gba.cpu().cpsr() & 0x1F, 0x13);
    let limit = 600 * CYCLES_PER_FRAME;
    while gba.cpu().reg(15) - 8 != 0x0800_0000 {
        assert!(
            gba.cycles() < limit,
            "the intro never reached the cartridge"
        );
        assert_eq!(gba.fault(), None);
        gba.step();
    }
    let skipped = Gba::new(&program);
    let state = gba.cpu().state();
    let expected = skipped.cpu().state();
    assert_eq!(gba.cpu().cpsr(), expected.cpsr);
    assert_eq!(state.r[13], expected.r[13]);
    assert_eq!(state.irq[0], expected.irq[0]);
    assert_eq!(state.svc[0], expected.svc[0]);
    assert_eq!(gba.bus().read16(0x0400_0000), 0x0080);
    assert_eq!(gba.bus().read8(0x0400_0300), 1);
}

fn through_the_intro(image: &[u8]) -> Gba {
    let mut gba = Gba::with_bios(image, Bios::built_in(), Boot::Intro);
    while gba.cpu().reg(15) - 8 != 0x0800_0000 {
        assert!(
            gba.cycles() < 300 * CYCLES_PER_FRAME,
            "the intro never reached the cartridge"
        );
        assert_eq!(gba.fault(), None);
        gba.step();
    }
    gba
}

#[test]
fn the_intro_hands_over_at_the_cycle_and_scanline_the_official_bios_does() {
    let gba = through_the_intro(&header_boot_byte(0x00));
    assert_eq!(gba.cycles(), 76_001_689);
    assert_eq!(gba.bus().read16(VCOUNT), 126);
    assert_eq!(gba.cycles() % CYCLES_PER_FRAME % 1232, 841);
}

#[test]
fn the_intro_stops_timer_0_where_the_official_bios_does() {
    let gba = through_the_intro(&header_boot_byte(0x00));
    assert_eq!(gba.bus().read16(0x0400_0100), 0xFF8A);
    assert_eq!(gba.bus().read16(0x0400_0102), 0);
    assert_eq!(gba.bus().timers().reload(0), 0);
}

#[test]
fn the_intro_starts_timer_0_a_cycle_earlier_when_header_byte_b4_has_bit_7_set() {
    let gba = through_the_intro(&header_boot_byte(0x80));
    assert_eq!(gba.bus().read16(0x0400_0100), 0xFF8B);
}

#[test]
fn the_intro_leaves_the_registers_the_official_bios_leaves() {
    let gba = through_the_intro(&rom(&[LOOP]));
    for (addr, value) in [
        (0x0400_0000, 0x0080),
        (0x0400_0004, 0),
        (0x0400_0082, 0x000E),
        (0x0400_0088, 0x0200),
        (0x0400_0134, 0x8000),
        (0x0400_0140, 0),
        (0x0400_0200, 0),
        (0x0400_0202, 0),
        (0x0400_0208, 0),
    ] {
        assert_eq!(gba.bus().read16(addr), value, "{addr:#010x}");
    }
    assert_eq!(gba.bus().read8(0x0400_0300), 1);
    assert_eq!(gba.bus().ppu().affine_matrix(0), [0x0100, 0, 0, 0x0100]);
    assert_eq!(gba.cpu().cpsr(), 0x1F);
    assert_eq!(gba.cpu().reg(13), 0x0300_7F00);
    assert_eq!(gba.cpu().reg(14), 0x0800_0000);
}

#[test]
fn the_intro_ends_in_the_state_the_skip_boot_starts_in() {
    for byte in [0x00, 0x80] {
        let image = header_boot_byte(byte);
        let intro = through_the_intro(&image);
        let skip = Gba::new(&image);
        assert_eq!(intro.cycles(), skip.cycles());
        assert_eq!(intro.cpu().state(), skip.cpu().state());
        for addr in (0x0400_0000..0x0400_0400).step_by(2) {
            assert_eq!(
                intro.bus().read16(addr),
                skip.bus().read16(addr),
                "{addr:#010x}"
            );
        }
        assert_eq!(
            intro.bus().timers().reload(0),
            skip.bus().timers().reload(0)
        );
    }
}

#[test]
fn soft_reset_clears_the_top_of_iwram_and_restarts_the_cartridge() {
    let mut gba = Gba::new(&rom(&[
        0xE3A0_6402,
        0xE596_7000,
        0xE357_0000,
        0x1A00_0002,
        0xE586_6000,
        0xE3A0_5007,
        0xEF00_0000,
        LOOP,
    ]));
    write_words(&mut gba, 0x0300_7DFC, &[5, 1]);
    gba.bus_mut().write32(0x0300_7F00, 0xDEAD);
    let limit = gba.cycles() + 4 * CYCLES_PER_FRAME;
    while !(gba.cpu().reg(15) - 8 == 0x0800_001C && reg(&gba, 7) != 0) {
        assert!(gba.cycles() < limit, "the cartridge did not restart");
        gba.step();
    }
    assert_eq!(reg(&gba, 5), 0);
    assert_eq!(reg(&gba, 13), 0x0300_7F00);
    assert_eq!(gba.cpu().cpsr() & 0x1F, 0x1F);
    assert_eq!(gba.bus().read32(0x0300_7DFC), 5);
    assert_eq!(gba.bus().read32(0x0300_7E00), 0);
    assert_eq!(gba.bus().read32(0x0300_7F00), 0);
}

#[test]
fn register_ram_reset_clears_the_memories_its_flags_name() {
    let fill = |gba: &mut Gba| {
        for addr in [EWRAM, IWRAM, 0x0300_7E00, 0x0500_0000, VRAM, 0x0700_0000] {
            gba.bus_mut().write32(addr, 0x1111_1111);
        }
    };
    let gba = call_with(0x01, [0x1E, 0, 0, 0], fill);
    assert_eq!(gba.bus().read32(EWRAM), 0x1111_1111);
    assert_eq!(gba.bus().read32(IWRAM), 0);
    assert_eq!(gba.bus().read32(0x0300_7E00), 0x1111_1111);
    assert_eq!(gba.bus().read32(0x0500_0000), 0);
    assert_eq!(gba.bus().read32(VRAM), 0);
    assert_eq!(gba.bus().read32(0x0700_0000), 0);
    let gba = call_with(0x01, [0x01, 0, 0, 0], fill);
    assert_eq!(gba.bus().read32(EWRAM), 0);
    assert_eq!(gba.bus().read32(IWRAM), 0x1111_1111);
}

#[test]
fn halt_sleeps_until_an_enabled_interrupt_is_flagged() {
    let gba = call_with(0x02, [0; 4], |gba| {
        gba.bus_mut().write16(0x0400_0004, 0x0008);
        gba.bus_mut().write16(0x0400_0200, 0x0001);
    });
    assert_eq!(gba.bus().read16(VCOUNT), 160);
}

fn install_irq_handler(gba: &mut Gba) {
    write_words(
        gba,
        IWRAM,
        &[
            0xE3A0_0301,
            0xE280_0C02,
            0xE1D0_10B2,
            0xE1C0_10B2,
            0xE240_0F82,
            0xE1D0_20B0,
            0xE182_2001,
            0xE1C0_20B0,
            0xE12F_FF1E,
        ],
    );
    gba.bus_mut().write32(IRQ_HANDLER, IWRAM);
    gba.bus_mut().write16(0x0400_0004, 0x0008);
    gba.bus_mut().write16(0x0400_0200, 0x0001);
    gba.bus_mut().write16(0x0400_0208, 0x0001);
}

#[test]
fn vblank_intr_wait_returns_after_the_next_vblank_irq() {
    let gba = call_with(0x05, [0; 4], install_irq_handler);
    assert_eq!(gba.bus().read16(VCOUNT), 160);
    assert_eq!(gba.bus().read16(IRQ_CHECK) & 1, 0);
}

#[test]
fn intr_wait_returns_at_once_for_a_flag_already_raised() {
    let gba = call_with(0x04, [0, 1, 0, 0], |gba| {
        install_irq_handler(gba);
        gba.bus_mut().write16(IRQ_CHECK, 1);
    });
    assert!(gba.bus().read16(VCOUNT) < 160);
    assert_eq!(gba.bus().read16(IRQ_CHECK), 0);
}

#[test]
fn intr_wait_discarding_old_flags_waits_for_a_new_irq() {
    let gba = call_with(0x04, [1, 1, 0, 0], |gba| {
        install_irq_handler(gba);
        gba.bus_mut().write16(IRQ_CHECK, 1);
    });
    assert_eq!(gba.bus().read16(VCOUNT), 160);
    assert_eq!(gba.bus().read16(IRQ_CHECK), 0);
}

#[test]
fn div_rounds_toward_zero_and_returns_the_remainder_and_absolute_quotient() {
    let gba = call(0x06, [-7i32 as u32, 2, 0, 0]);
    assert_eq!(reg(&gba, 0) as i32, -3);
    assert_eq!(reg(&gba, 1) as i32, -1);
    assert_eq!(reg(&gba, 3), 3);
    let gba = call(0x06, [100, -7i32 as u32, 0, 0]);
    assert_eq!(reg(&gba, 0) as i32, -14);
    assert_eq!(reg(&gba, 1) as i32, 2);
    assert_eq!(reg(&gba, 3), 14);
}

#[test]
fn div_arm_swaps_numerator_and_denominator() {
    let gba = call(0x07, [2, -7i32 as u32, 0, 0]);
    assert_eq!(reg(&gba, 0) as i32, -3);
    assert_eq!(reg(&gba, 1) as i32, -1);
    assert_eq!(reg(&gba, 3), 3);
}

#[test]
fn sqrt_returns_the_integer_square_root() {
    for (value, root) in [
        (0, 0),
        (15, 3),
        (16, 4),
        (1_000_000, 1000),
        (u32::MAX, 0xFFFF),
    ] {
        assert_eq!(reg(&call(0x08, [value, 0, 0, 0]), 0), root, "sqrt({value})");
    }
}

#[test]
fn arc_tan_maps_minus_pi_half_to_pi_half_onto_c000_to_4000() {
    for tan in [0.0, 0.25, 0.5, 1.0, -1.0, -0.75] {
        let input = (tan * 16384.0) as i32 as u32;
        let result = reg(&call(0x09, [input, 0, 0, 0]), 0) as i16 as i32;
        let expected = angle(f64::atan(tan) / FRAC_PI_2);
        assert!(
            (result - expected).abs() <= 4,
            "atan({tan}) = {result:#x}, wanted {expected:#x}"
        );
    }
}

#[test]
fn arc_tan2_maps_the_full_circle_onto_0000_to_ffff() {
    for (x, y) in [
        (1.0, 0.0),
        (1.0, 1.0),
        (0.0, 1.0),
        (-1.0, 0.0),
        (-0.5, -0.5),
        (0.5, -1.0),
    ] {
        let fixed = |v: f64| (v * 16384.0) as i32 as u32;
        let result = reg(&call(0x0A, [fixed(x), fixed(y), 0, 0]), 0) as i32;
        let turn = f64::atan2(y, x).rem_euclid(4.0 * FRAC_PI_2) / FRAC_PI_2;
        let expected = angle(turn) & 0xFFFF;
        assert!(
            (result - expected).abs() <= 4,
            "atan2({y}, {x}) = {result:#x}, wanted {expected:#x}"
        );
    }
}

#[test]
fn cpu_set_copies_halfwords_and_fills_words() {
    let source = EWRAM;
    let dest = EWRAM + 0x100;
    let gba = call_with(0x0B, [source, dest, 3, 0], |gba| {
        write_bytes(gba, source, &[1, 2, 3, 4, 5, 6, 7, 8]);
    });
    assert_eq!(read_bytes(&gba, dest, 8), [1, 2, 3, 4, 5, 6, 0, 0]);
    let gba = call_with(0x0B, [source, dest, 3 | 1 << 24 | 1 << 26, 0], |gba| {
        write_words(gba, source, &[0xAABB_CCDD, 0x1234_5678]);
    });
    for i in 0..3 {
        assert_eq!(gba.bus().read32(dest + 4 * i), 0xAABB_CCDD);
    }
    assert_eq!(gba.bus().read32(dest + 12), 0);
}

#[test]
fn cpu_fast_set_copies_and_fills_blocks_of_eight_words() {
    let source = EWRAM;
    let dest = EWRAM + 0x100;
    let words: Vec<u32> = (1..=9).collect();
    let gba = call_with(0x0C, [source, dest, 8, 0], |gba| {
        write_words(gba, source, &words)
    });
    for i in 0..8 {
        assert_eq!(gba.bus().read32(dest + 4 * i), i + 1);
    }
    assert_eq!(gba.bus().read32(dest + 32), 0);
    let gba = call_with(0x0C, [source, dest, 8 | 1 << 24, 0], |gba| {
        write_words(gba, source, &words)
    });
    for i in 0..8 {
        assert_eq!(gba.bus().read32(dest + 4 * i), 1);
    }
    assert_eq!(gba.bus().read32(dest + 32), 0);
}

fn bg_affine(angle: u16) -> Gba {
    let source = EWRAM;
    let dest = EWRAM + 0x100;
    call_with(0x0E, [source, dest, 1, 0], |gba| {
        write_words(gba, source, &[0x1000, 0x2000]);
        for (i, value) in [8u16, 4, 0x100, 0x200, angle].into_iter().enumerate() {
            gba.bus_mut().write16(source + 8 + 2 * i as u32, value);
        }
    })
}

#[test]
fn bg_affine_set_scales_and_moves_the_origin() {
    let gba = bg_affine(0);
    let dest = EWRAM + 0x100;
    let params: Vec<i16> = (0..4).map(|i| half(&gba, dest + 2 * i)).collect();
    assert_eq!(params, [0x100, 0, 0, 0x200]);
    assert_eq!(gba.bus().read32(dest + 8) as i32, 0x1000 - 0x100 * 8);
    assert_eq!(gba.bus().read32(dest + 12) as i32, 0x2000 - 0x200 * 4);
}

#[test]
fn bg_affine_set_rotates_a_quarter_turn() {
    let gba = bg_affine(0x4000);
    let dest = EWRAM + 0x100;
    let params: Vec<i16> = (0..4).map(|i| half(&gba, dest + 2 * i)).collect();
    assert_eq!(params, [0, -0x100, 0x200, 0]);
    assert_eq!(gba.bus().read32(dest + 8) as i32, 0x1000 + 0x100 * 4);
    assert_eq!(gba.bus().read32(dest + 12) as i32, 0x2000 - 0x200 * 8);
}

#[test]
fn obj_affine_set_writes_with_the_given_stride() {
    let source = EWRAM;
    let dest = EWRAM + 0x100;
    let gba = call_with(0x0F, [source, dest, 2, 8], |gba| {
        for (i, value) in [0x100u16, 0x80, 0, 0, 0x100, 0x80, 0x4000, 0]
            .into_iter()
            .enumerate()
        {
            gba.bus_mut().write16(source + 2 * i as u32, value);
        }
    });
    let params: Vec<i16> = (0..8).map(|i| half(&gba, dest + 8 * i)).collect();
    assert_eq!(params, [0x100, 0, 0, 0x80, 0, -0x100, 0x80, 0]);
}

const LZ77_ABC: [u8; 10] = [0x10, 12, 0, 0, 0x10, b'A', b'B', b'C', 0x60, 0x02];
const RL_AB: [u8; 8] = [0x30, 6, 0, 0, 0x82, b'A', 0x00, b'B'];
const HUFFMAN_ABBA: [u8; 12] = [0x28, 4, 0, 0, 1, 0xC0, b'A', b'B', 0, 0, 0, 0x60];

fn decompress(number: u32, data: &[u8], dest: u32, len: u32) -> Vec<u8> {
    let gba = call_with(number, [EWRAM, dest, 0, 0], |gba| {
        write_bytes(gba, EWRAM, data)
    });
    read_bytes(&gba, dest, len)
}

#[test]
fn lz77_expands_literals_and_overlapping_back_references() {
    let want = b"ABCABCABCABC\0\0\0\0".to_vec();
    assert_eq!(decompress(0x11, &LZ77_ABC, EWRAM + 0x100, 16), want);
    assert_eq!(decompress(0x12, &LZ77_ABC, VRAM, 16), want);
}

#[test]
fn run_length_expands_runs_and_raw_bytes() {
    let want = b"AAAAAB\0\0".to_vec();
    assert_eq!(decompress(0x14, &RL_AB, EWRAM + 0x100, 8), want);
    assert_eq!(decompress(0x15, &RL_AB, VRAM, 8), want);
}

#[test]
fn huffman_walks_the_tree_from_the_top_bit_down() {
    assert_eq!(
        decompress(0x13, &HUFFMAN_ABBA, EWRAM + 0x100, 8),
        b"ABBA\0\0\0\0"
    );
}

#[test]
fn the_console_powers_the_video_unit_on_at_line_225() {
    let gba = Gba::with_bios(&rom(&[LOOP]), Bios::built_in(), Boot::Intro);
    assert_eq!(gba.cycles(), 0);
    assert_eq!(gba.bus().read16(VCOUNT), 225);
    assert_eq!(gba.bus().read16(0x0400_0004) & 1, 1);
}

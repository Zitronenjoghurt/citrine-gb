use super::*;
use crate::console::{Console, ConsoleKind};

const LOOP: u32 = 0xEAFF_FFFE;

fn rom(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

fn timer_irq_program() -> Vec<u8> {
    rom(&[
        0xE59F_0030,
        0xE59F_1030,
        0xE580_1000,
        0xE59F_002C,
        0xE59F_102C,
        0xE580_1100,
        0xE280_2C02,
        0xE3A0_1008,
        0xE582_1000,
        0xE3A0_1001,
        0xE582_1008,
        LOOP,
        0,
        0,
        0x0300_7FFC,
        0x0800_0048,
        0x0400_0000,
        0x00C0_0000,
        0xE285_5001,
        0xE3A0_0301,
        0xE280_0C02,
        0xE3A0_1008,
        0xE1C0_10B2,
        0xE12F_FF1E,
    ])
}

#[test]
fn boots_at_the_cartridge_entry_in_system_mode() {
    let gba = Gba::new(&rom(&[LOOP, 0xE3A0_0001]));
    let state = gba.cpu().state();
    assert_eq!(gba.cpu().cpsr(), 0x1F);
    assert_eq!(state.r[13], 0x0300_7F00);
    assert_eq!(state.irq[0], 0x0300_7FA0);
    assert_eq!(state.svc[0], 0x0300_7FE0);
    assert_eq!(state.r[15], 0x0800_0008);
    assert_eq!(state.pipeline, [LOOP, 0xE3A0_0001]);
}

#[test]
fn a_frame_is_280896_cycles_and_overshoot_carries_into_the_next() {
    let mut gba = Gba::new(&rom(&[LOOP]));
    let boundary = gba.cycles().next_multiple_of(CYCLES_PER_FRAME);
    gba.run_frame();
    let first = gba.cycles();
    assert!((boundary..boundary + 32).contains(&first), "{first}");
    gba.run_frame();
    let next = boundary + CYCLES_PER_FRAME;
    assert!((next..next + 32).contains(&gba.cycles()));
}

#[test]
fn a_frame_after_stepping_past_several_frames_runs_a_whole_frame() {
    let mut gba = Gba::new(&rom(&[LOOP]));
    let base = gba.cycles().next_multiple_of(CYCLES_PER_FRAME);
    while gba.cycles() < base + 2 * CYCLES_PER_FRAME + 100 {
        gba.step();
    }
    gba.run_frame();
    let end = gba.cycles();
    let third = base + 3 * CYCLES_PER_FRAME;
    assert!((third..third + 32).contains(&end), "{end}");
    gba.run_frame();
    let fourth = third + CYCLES_PER_FRAME;
    assert!((fourth..fourth + 32).contains(&gba.cycles()));
}

#[test]
fn an_irq_calls_the_handler_at_03007ffc_and_returns_to_the_program() {
    let mut gba = Gba::new(&timer_irq_program());
    let end = gba.cycles() + CYCLES_PER_FRAME;
    while gba.cycles() < end {
        gba.step();
    }
    assert_eq!(gba.fault(), None);
    assert_eq!(gba.cpu().reg(5), 4);
    assert_eq!(gba.cpu().cpsr() & 0x1F, 0x1F);
    assert_eq!(gba.cpu().reg(15) - 8, 0x0800_002C);
}

#[test]
fn an_irq_enters_through_the_bios_handler_at_0128() {
    let mut gba = Gba::new(&timer_irq_program());
    let mut seen = Vec::new();
    let limit = gba.cycles() + CYCLES_PER_FRAME;
    while gba.cycles() < limit && seen.len() < 3 {
        gba.step();
        let pc = gba.cpu().reg(15) - 8;
        if pc < 0x4000 && seen.last() != Some(&pc) {
            seen.push(pc);
        }
    }
    assert_eq!(seen, [0x18, 0x128, 0x12C]);
}

#[test]
fn an_swi_runs_the_bios_call_and_returns() {
    let mut gba = Gba::new(&rom(&[0xE3A0_0E19, 0xEF08_0000, LOOP]));
    gba.run_frame();
    assert_eq!(gba.fault(), None);
    assert_eq!(gba.cpu().reg(0), 20);
    assert_eq!(gba.cpu().cpsr() & 0x1F, 0x1F);
    assert_eq!(gba.cpu().reg(15) - 8, 0x0800_0008);
}

#[test]
fn haltcnt_sleeps_until_ie_and_if_meet_with_ime_off() {
    let mut gba = Gba::new(&rom(&[
        0xE3A0_0301,
        0xE3A0_1008,
        0xE1C0_10B4,
        0xE3A0_1001,
        0xE280_2C02,
        0xE1C2_10B0,
        0xE3A0_1000,
        0xE5C0_1301,
        0xE1D0_50B6,
        LOOP,
    ]));
    gba.step();
    let limit = gba.cycles() + CYCLES_PER_FRAME;
    while gba.power() == Power::Running && gba.cycles() < limit {
        gba.step();
    }
    assert_eq!(gba.power(), Power::Halted);
    gba.run_frame();
    assert_eq!(gba.power(), Power::Running);
    assert_eq!(gba.cpu().reg(5), 160);
    assert_eq!(gba.cpu().cpsr() & 0x1F, 0x1F);
}

#[test]
fn stop_freezes_video_and_timers_until_a_key_irq() {
    let mut gba = Gba::new(&rom(&[0xE3A0_0301, 0xE3A0_1080, 0xE5C0_1301, LOOP]));
    gba.bus_mut().write16(0x0400_0132, 0x4001);
    gba.bus_mut().write16(0x0400_0200, 0x1000);
    while gba.power() != Power::Stopped {
        gba.step();
    }
    let vcount = gba.bus().read16(0x0400_0006);
    gba.run_frame();
    assert_eq!(gba.power(), Power::Stopped);
    assert_eq!(gba.bus().read16(0x0400_0006), vcount);
    gba.set_keys(Keys::A);
    assert_eq!(gba.step(), 2);
    assert_eq!(gba.power(), Power::Stopped);
    gba.step();
    assert_eq!(gba.power(), Power::Running);
}

#[test]
fn soft_reset_boots_the_same_cartridge_again() {
    let mut gba = Gba::new(&rom(&[0xEF05_0000]));
    gba.run_frame();
    gba.soft_reset();
    assert_eq!(gba.fault(), None);
    assert_eq!(gba.cycles(), Boot::Skip.entry_cycles());
    assert_eq!(gba.cpu().state().pipeline[0], 0xEF05_0000);
}

#[test]
fn soft_reset_keeps_the_sample_rate_and_muted_channels() {
    let mut gba = Gba::new(&rom(&[LOOP]));
    gba.set_sample_rate(32_000);
    gba.set_channel_enabled(4, false);
    gba.soft_reset();
    assert_eq!(Console::sample_rate(&gba), 32_000);
    assert!(!gba.bus().sound().channel_enabled(4));
    assert!(gba.bus().sound().channel_enabled(0));
}

#[test]
fn a_frame_gives_stereo_audio_at_the_sample_rate() {
    let mut gba = Gba::new(&rom(&[LOOP]));
    gba.set_sample_rate(48_000);
    let mut out = Vec::new();
    gba.run_frame();
    gba.take_audio(&mut out);
    out.clear();
    gba.run_frame();
    gba.take_audio(&mut out);
    assert_eq!(out.len() % 2, 0);
    assert!((1600..=1612).contains(&out.len()), "{}", out.len());
}

#[test]
fn the_console_names_the_six_sound_channels() {
    let gba = Gba::new(&rom(&[LOOP]));
    assert_eq!(gba.audio_channels().len(), 6);
    assert_eq!(gba.audio_channels()[4], "Direct Sound A");
}

#[test]
fn the_console_is_a_240_by_160_gba() {
    let gba = Gba::new(&rom(&[LOOP]));
    assert_eq!(gba.kind(), ConsoleKind::Gba);
    assert_eq!(Console::video(&gba).width, 240);
    assert_eq!(Console::video(&gba).height, 160);
    assert_eq!(Console::frame(&gba).len(), 240 * 160 * 4);
    assert_eq!(gba.timing().cycles_per_frame, 280_896);
}

#[test]
fn the_title_is_the_header_name_up_to_its_first_zero() {
    let mut data = rom(&[LOOP]);
    data.resize(0xC0, 0);
    data[0xA0..0xA7].copy_from_slice(b"TOBU GB");
    let gba = Gba::new(&data);
    assert_eq!(gba.title(), "TOBU GB");
    assert_eq!(gba.rom_bytes(), data);
    assert!(gba.has_battery());
    assert!(!gba.has_clock());
}

#[cfg(feature = "persistence")]
fn sram_game() -> Gba {
    let mut data = rom(&[LOOP]);
    data.extend_from_slice(b"SRAM_V113");
    data.resize(0x200, 0);
    Gba::new(&data)
}

#[cfg(feature = "persistence")]
#[test]
fn a_write_to_the_backup_is_saved_once() {
    let mut gba = sram_game();
    assert!(gba.battery(false, 0).is_none());
    gba.bus.write8(0x0E00_0010, 0x42);
    let save = gba.battery(false, 0).expect("written");
    assert_eq!(save.ram.len(), 0x8000);
    assert_eq!(save.ram[0x10], 0x42);
    assert_eq!(save.clock, None);
    assert!(gba.battery(false, 0).is_none());
    assert_eq!(gba.battery(true, 0).expect("forced").ram, save.ram);
}

#[cfg(feature = "persistence")]
#[test]
fn a_forced_read_leaves_the_write_for_the_next_save() {
    let mut gba = sram_game();
    gba.bus.write8(0x0E00_0010, 0x42);
    assert_eq!(gba.battery(true, 0).expect("forced").ram[0x10], 0x42);
    let save = gba.battery(false, 0).expect("still unsaved");
    assert_eq!(save.ram[0x10], 0x42);
    assert!(gba.battery(false, 0).is_none());
}

#[cfg(feature = "persistence")]
#[test]
fn a_put_battery_comes_back_from_the_backup() {
    let mut gba = sram_game();
    let mut ram = vec![0xFF; 0x8000];
    ram[3] = 7;
    gba.put_battery(gba.battery_from_file(&ram), 0);
    assert_eq!(gba.bus.read8(0x0E00_0003), 7);
    assert_eq!(gba.battery(true, 0).expect("forced").ram, ram);
}

#[cfg(feature = "persistence")]
#[test]
fn a_game_that_never_saved_has_nothing_to_store() {
    let mut gba = Gba::new(&rom(&[LOOP]));
    assert!(gba.battery(true, 0).is_none());
}

#[test]
fn a_short_rom_has_no_title() {
    assert_eq!(Gba::new(&rom(&[LOOP])).title(), "");
}

#[cfg(feature = "persistence")]
#[test]
fn a_gba_state_carries_the_magic_and_resumes_where_it_was_saved() {
    use crate::persistence::state::{GBA_STATE_MAGIC, GBA_STATE_VERSION};
    let program = timer_irq_program();
    let mut gba = Gba::new(&program);
    gba.bus_mut().write8(0x0E00_0010, 0x5A);
    for _ in 0..3 {
        gba.run_frame();
    }
    let state = gba.save_state(0).expect("state");
    assert_eq!(state[..4], GBA_STATE_MAGIC);
    assert_eq!(state[4], GBA_STATE_VERSION);

    let mut restored = Gba::new(&program).restore(&state).expect("restore");
    for _ in 0..5 {
        gba.run_frame();
        restored.run_frame();
    }
    assert_eq!(restored.cycles(), gba.cycles());
    assert_eq!(restored.frame(), gba.frame());
    assert_eq!(restored.bus().backup().bytes(), gba.bus().backup().bytes());
    assert_eq!(restored.bus().read8(0x0E00_0010), 0x5A);
}

#[cfg(feature = "persistence")]
#[test]
fn a_gba_state_from_a_newer_build_is_refused() {
    let mut gba = Gba::new(&rom(&[LOOP]));
    let mut state = gba.save_state(0).expect("state");
    state[4] += 1;
    assert!(matches!(
        gba.restore(&state),
        Err(crate::error::GbError::NewerState)
    ));
}

#[cfg(feature = "persistence")]
#[test]
fn a_gba_state_loads_as_a_gba_machine() {
    use crate::machine::{Firmware, Machine};
    use crate::rom::Rom;
    let program = timer_irq_program();
    let mut machine = Machine::Gba(Box::new(Gba::new(&program)));
    machine.run_frame();
    let state = machine.save_state(0).expect("state");
    let firmware = Firmware::default();
    let loaded =
        Machine::from_state(&state, &Rom::new(&program), 0, &firmware).expect("from state");
    assert_eq!(loaded.kind(), ConsoleKind::Gba);
    let mut restored = machine.restore(&state, 0, &firmware).expect("restore");
    restored.run_frame();
    machine.run_frame();
    assert_eq!(restored.frame(), machine.frame());
}

#[cfg(feature = "persistence")]
#[test]
fn a_gba_state_loads_with_the_user_bios() {
    use crate::gba::bios::{Bios, BiosKind, Boot, CULT_OF_GBA};
    use crate::machine::{Firmware, Machine};
    use crate::rom::Rom;
    let program = timer_irq_program();
    let state = Machine::Gba(Box::new(Gba::new(&program)))
        .save_state(0)
        .expect("state");
    let firmware = Firmware {
        gba_bios: Some(Bios::user(CULT_OF_GBA).expect("16 KB")),
        gba_boot: Boot::Skip,
        ..Firmware::default()
    };
    let Machine::Gba(loaded) =
        Machine::from_state(&state, &Rom::new(&program), 0, &firmware).expect("from state")
    else {
        panic!("not a gba");
    };
    assert_eq!(loaded.bios(), BiosKind::User);
}

#[cfg(feature = "persistence")]
#[test]
fn a_battery_save_names_its_chip_and_settles_an_undetected_cartridge() {
    use crate::gba::save::SaveType;
    let program = rom(&[LOOP]);
    let mut gba = Gba::new(&program);
    gba.bus_mut().write8(0x0E00_0001, 0x42);
    let save = gba.battery(false, 0).expect("save");
    assert_eq!(save.chip, Some(SaveType::Sram32K));

    let mut fresh = Gba::new(&program);
    let mut flash = save.clone();
    flash.chip = Some(SaveType::Flash64K);
    flash.ram = vec![0x11; 0x1_0000];
    fresh.put_battery(flash, 0);
    assert_eq!(fresh.bus().backup().kind(), SaveType::Flash64K);
    assert_eq!(fresh.bus().read8(0x0E00_0123), 0x11);
}

#[test]
fn the_screen_starts_in_forced_blank() {
    let mut gba = Gba::new(&rom(&[LOOP]));
    gba.run_frame();
    assert!(gba.frame().iter().all(|&byte| byte == 0xFF));
}

#[test]
fn a_program_that_draws_in_mode_3_shows_its_pixel() {
    let mut gba = Gba::new(&rom(&[
        0xE3A0_0301,
        0xE3A0_1B01,
        0xE381_1003,
        0xE1C0_10B0,
        0xE3A0_0406,
        0xE3A0_101F,
        0xE1C0_10B0,
        LOOP,
    ]));
    gba.run_frame();
    gba.run_frame();
    assert_eq!(gba.frame()[..8], [0xFF, 0, 0, 0xFF, 0, 0, 0, 0xFF]);
}

#[test]
fn a_timer_read_counts_the_cycles_of_its_own_instruction() {
    let mut gba = Gba::new(&rom(&[
        0xE3A0_3301,
        0xE3A0_0502,
        0xE583_0100,
        0xE593_1100,
        LOOP,
    ]));
    for _ in 0..4 {
        gba.step();
    }
    assert_eq!(gba.cpu().reg(1), 0x0080_0007);
}

#[cfg(feature = "persistence")]
#[test]
fn a_gba_state_saves_in_a_small_part_of_the_slowest_brotli() {
    use crate::persistence::state::SAVE_QUALITY;
    let mut gba = Gba::new(&rom(&[LOOP]));
    let mut x = 0x1234_5678u32;
    for offset in 0..0x4_0000u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let value = if offset % 64 < 40 { (x % 12) as u8 } else { 0 };
        gba.bus_mut().write8(0x0200_0000 + offset, value);
    }
    let started = std::time::Instant::now();
    let state = gba.save_state(0).expect("state");
    let saved = started.elapsed();
    let started = std::time::Instant::now();
    gba.state_body(0)
        .expect("body")
        .pack(SAVE_QUALITY)
        .expect("pack");
    let slowest = started.elapsed();
    assert!(
        saved * 10 < slowest,
        "saved in {saved:?}, the slowest brotli took {slowest:?}"
    );
    assert!(Gba::new(&rom(&[LOOP])).restore(&state).is_ok());
}

#[test]
fn a_halt_that_a_dma_ends_runs_the_held_instruction_as_the_dma_lets_go() {
    let mut gba = Gba::new(&rom(&[
        0xE3A0_0301,
        0xE3A0_1010,
        0xE1C0_10B4,
        0xE280_2C02,
        0xE3A0_1002,
        0xE1C2_10B0,
        0xE280_30D4,
        0xE3A0_1302,
        0xE583_1000,
        0xE3A0_1402,
        0xE583_1004,
        0xE3A0_1C02,
        0xE1C3_10B8,
        0xE3A0_1CA4,
        0xE1C3_10BA,
        0xE5C0_1301,
        0xE3A0_5001,
        LOOP,
    ]));
    let limit = gba.cycles() + CYCLES_PER_FRAME;
    while gba.power() == Power::Running && gba.cycles() < limit {
        gba.step();
    }
    while !gba.bus().dma_active() && gba.cycles() < limit {
        gba.step();
    }
    assert_eq!(gba.power(), Power::Halted);
    gba.step();
    assert_eq!(gba.power(), Power::Running);
    assert_eq!(gba.cpu().reg(5), 1);
}

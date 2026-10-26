use crate::gb::bus::CpuBusInterface;
use crate::gb::ppu::types::mode::PpuMode;
use crate::gb::{GameBoy, GbModel};
use crate::rom::Rom;
use crate::rom::header::RomCgbMode;
use crate::{ReadMemory, WriteMemory};

fn rom(cgb_flag: u8, code: &[u8]) -> Rom {
    let mut data = vec![0u8; 0x8000];
    data[0x0100..0x0104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]);
    data[0x0143] = cgb_flag;
    data[0x0150..0x0150 + code.len()].copy_from_slice(code);
    let end = 0x0150 + code.len();
    data[end..end + 2].copy_from_slice(&[0x18, 0xFE]);
    Rom::new(&data)
}

fn boot(model: GbModel, cgb_flag: u8, code: &[u8]) -> GameBoy {
    let mut gb = GameBoy::new_empty(model);
    gb.load_rom(&rom(cgb_flag, code)).expect("load");
    gb
}

fn read(gb: &mut GameBoy, addr: u16) -> u8 {
    gb.with_bus(|bus| bus.read_naive(addr))
}

fn write(gb: &mut GameBoy, addr: u16, value: u8) {
    gb.with_bus(|bus| bus.write_naive(addr, value))
}

fn cycles(gb: &mut GameBoy, count: u32) {
    gb.with_bus(|bus| {
        for _ in 0..count {
            bus.cycle();
        }
    })
}

#[test]
fn header_flag_picks_the_model() {
    for (flag, mode, model) in [
        (0x00, RomCgbMode::None, GbModel::Dmg),
        (0x80, RomCgbMode::CgbAndGb, GbModel::Cgb),
        (0xC0, RomCgbMode::CgbOnly, GbModel::Cgb),
    ] {
        let rom = rom(flag, &[]);
        assert_eq!(rom.cgb_mode().unwrap(), mode);
        assert_eq!(GbModel::for_rom(&rom).unwrap(), model);
    }
}

#[test]
fn load_rom_with_model_rebuilds_the_machine() {
    let mut gb = GameBoy::new_empty(GbModel::Dmg);
    gb.load_rom_with_model(&rom(0x80, &[]), GbModel::Cgb)
        .unwrap();
    assert_eq!(gb.model, GbModel::Cgb);
    assert_eq!(gb.cpu.a, 0x11);
    gb.soft_reset();
    assert_eq!(gb.model, GbModel::Cgb);
    assert_eq!(read(&mut gb, 0xFF4D), 0x7E);
}

#[test]
fn cgb_game_starts_in_cgb_mode() {
    let mut gb = boot(GbModel::Cgb, 0x80, &[]);
    assert!(gb.cgb.cgb_mode);
    assert_eq!(
        [gb.cpu.a, gb.cpu.d, gb.cpu.e, gb.cpu.l],
        [0x11, 0xFF, 0x56, 0x0D]
    );
    assert_eq!(read(&mut gb, 0xFF4D), 0x7E);
    assert_eq!(read(&mut gb, 0xFF55), 0xFF);
    assert_eq!(read(&mut gb, 0xFF70), 0xF8);
    assert_eq!(read(&mut gb, 0xFF02), 0x7E);
}

#[test]
fn sound_registers_match_the_boot_rom_handover() {
    for model in [GbModel::Dmg, GbModel::Cgb] {
        let mut gb = boot(model, 0x00, &[]);
        let sound: Vec<u8> = [0xFF11, 0xFF12, 0xFF24, 0xFF25, 0xFF26]
            .into_iter()
            .map(|addr| read(&mut gb, addr))
            .collect();
        assert_eq!(sound, [0xBF, 0xF3, 0x77, 0xF3, 0xF1]);
    }
}

#[test]
fn compatibility_mode_hides_the_colour_palettes() {
    let mut gb = boot(GbModel::Cgb, 0x00, &[]);
    assert_eq!(read(&mut gb, 0xFF00), 0xFF);
    assert_eq!(read(&mut gb, 0xFF68), 0xC8);
    assert_eq!(read(&mut gb, 0xFF6A), 0xD0);
    for addr in [0xFF69, 0xFF6B, 0xFF6C] {
        write(&mut gb, addr, 0x00);
        assert_eq!(read(&mut gb, addr), 0xFF);
    }
}

#[test]
fn dmg_game_on_cgb_runs_in_compatibility_mode() {
    let mut gb = boot(GbModel::Cgb, 0x00, &[]);
    assert!(!gb.cgb.cgb_mode);
    assert_eq!(
        [gb.cpu.a, gb.cpu.b, gb.cpu.d, gb.cpu.e, gb.cpu.h, gb.cpu.l],
        [0x11, 0x00, 0x00, 0x08, 0x00, 0x7C]
    );
    for addr in [0xFF4D, 0xFF55, 0xFF56, 0xFF70, 0xFF74] {
        write(&mut gb, addr, 0x00);
        assert_eq!(read(&mut gb, addr), 0xFF, "{addr:04X}");
    }
    assert_eq!(read(&mut gb, 0xFF75), 0x8F);
    assert_eq!(read(&mut gb, 0xFF72), 0x00);
}

#[test]
fn dmg_model_has_no_cgb_registers() {
    let mut gb = boot(GbModel::Dmg, 0x80, &[]);
    for addr in [0xFF4D, 0xFF55, 0xFF56, 0xFF70, 0xFF72, 0xFF75, 0xFF76] {
        assert_eq!(read(&mut gb, addr), 0xFF, "{addr:04X}");
    }
}

#[test]
fn svbk_switches_the_upper_wram_bank() {
    let mut gb = boot(GbModel::Cgb, 0x80, &[]);
    for bank in 1..8 {
        write(&mut gb, 0xFF70, bank);
        write(&mut gb, 0xD000, bank * 0x11);
    }
    write(&mut gb, 0xFF70, 0);
    assert_eq!(read(&mut gb, 0xD000), 0x11);
    write(&mut gb, 0xFF70, 5);
    assert_eq!(read(&mut gb, 0xD000), 0x55);
    assert_eq!(read(&mut gb, 0xF000), 0x55);
    assert_eq!(read(&mut gb, 0xFF70), 0xFD);
}

#[test]
fn serial_transfer_with_nothing_connected() {
    let mut gb = boot(GbModel::Dmg, 0x00, &[]);
    write(&mut gb, 0xFF0F, 0x00);
    write(&mut gb, 0xFF01, 0x55);
    write(&mut gb, 0xFF02, 0x81);
    assert_eq!(gb.serial.take_sent(), Some(0x55));
    cycles(&mut gb, 8 * 128 - 130);
    assert_eq!(read(&mut gb, 0xFF02) & 0x80, 0x80);
    cycles(&mut gb, 130);
    assert_eq!(read(&mut gb, 0xFF02), 0x7F);
    assert_eq!(read(&mut gb, 0xFF01), 0xFF);
    assert_eq!(read(&mut gb, 0xFF0F) & 0x08, 0x08);
}

#[test]
fn cgb_fast_serial_clock() {
    let mut gb = boot(GbModel::Cgb, 0x80, &[]);
    write(&mut gb, 0xFF02, 0x83);
    cycles(&mut gb, 8 * 4 + 2);
    assert_eq!(read(&mut gb, 0xFF02), 0x7F);
    assert_eq!(read(&mut gb, 0xFF01), 0xFF);
}

#[test]
fn stop_with_key1_armed_switches_speed() {
    let code = [0x3E, 0x01, 0xE0, 0x4D, 0x10, 0x00];
    let mut gb = boot(GbModel::Cgb, 0x80, &code);
    for _ in 0..20 {
        if gb.cgb.double_speed {
            break;
        }
        gb.step();
    }
    assert!(gb.cgb.double_speed);
    assert!(gb.cpu.halted);
    assert_eq!(read(&mut gb, 0xFF4D), 0xFE);
    assert!(read(&mut gb, 0xFF04) < 2);
    assert_eq!(gb.frame_cycles(), 2 * 17556);
}

#[test]
fn ppu_runs_at_the_same_rate_in_both_speeds() {
    for double_speed in [false, true] {
        let mut gb = boot(GbModel::Cgb, 0x80, &[]);
        gb.cgb.double_speed = double_speed;
        while gb.ppu.ly != 10 {
            cycles(&mut gb, 1);
        }
        let mut count = 0;
        while gb.ppu.ly != 11 {
            cycles(&mut gb, 1);
            count += 1;
        }
        assert_eq!(count, if double_speed { 228 } else { 114 });
    }
}

fn fill_source(gb: &mut GameBoy, len: u16) {
    for i in 0..len {
        write(gb, 0xC000 + i, i as u8 ^ 0x5A);
    }
    write(gb, 0xFF51, 0xC0);
    write(gb, 0xFF52, 0x0F);
    write(gb, 0xFF53, 0x81);
    write(gb, 0xFF54, 0x0F);
}

#[test]
fn general_purpose_dma_copies_everything_at_once() {
    let mut gb = boot(GbModel::Cgb, 0x80, &[]);
    fill_source(&mut gb, 0x40);
    write(&mut gb, 0xFF55, 0x03);
    let before = gb.cycle_counter;
    gb.with_bus(|bus| bus.service_hdma());
    assert_eq!(gb.cycle_counter - before, 4 * 9);
    for i in 0..0x40u16 {
        assert_eq!(read(&mut gb, 0x8100 + i), i as u8 ^ 0x5A);
    }
    assert_eq!(read(&mut gb, 0xFF55), 0xFF);
}

#[test]
fn hblank_dma_copies_one_block_per_hblank() {
    let mut gb = boot(GbModel::Cgb, 0x80, &[]);
    while gb.ppu.stat.ppu_mode != PpuMode::OamScan {
        cycles(&mut gb, 1);
    }
    fill_source(&mut gb, 0x30);
    write(&mut gb, 0xFF55, 0x82);
    assert_eq!(read(&mut gb, 0xFF55), 0x02);

    for block in 0..3u16 {
        while !gb.hdma.is_pending() {
            cycles(&mut gb, 1);
        }
        gb.with_bus(|bus| bus.service_hdma());
        assert_eq!(
            read(&mut gb, 0x8100 + block * 16 + 15),
            (block * 16 + 15) as u8 ^ 0x5A
        );
        assert_eq!(read(&mut gb, 0x8100 + block * 16 + 16), 0x00);
    }
    assert_eq!(read(&mut gb, 0xFF55), 0xFF);
}

#[test]
fn hblank_dma_can_be_cancelled() {
    let mut gb = boot(GbModel::Cgb, 0x80, &[]);
    while gb.ppu.stat.ppu_mode != PpuMode::OamScan {
        cycles(&mut gb, 1);
    }
    fill_source(&mut gb, 0x30);
    write(&mut gb, 0xFF55, 0x82);
    write(&mut gb, 0xFF55, 0x00);
    assert_eq!(read(&mut gb, 0xFF55), 0x80);
    cycles(&mut gb, 456);
    assert!(!gb.hdma.is_pending());
}

#[test]
fn hblank_dma_waits_while_halted() {
    let mut gb = boot(GbModel::Cgb, 0x80, &[]);
    while gb.ppu.stat.ppu_mode != PpuMode::OamScan {
        cycles(&mut gb, 1);
    }
    fill_source(&mut gb, 0x10);
    write(&mut gb, 0xFF55, 0x80);
    gb.with_bus(|bus| {
        while bus.ppu.stat.ppu_mode != PpuMode::HBlank {
            bus.cycle_halted(4);
        }
    });
    assert!(!gb.hdma.is_pending());
    gb.with_bus(|bus| bus.halt_ended());
    assert!(gb.hdma.is_pending());
}

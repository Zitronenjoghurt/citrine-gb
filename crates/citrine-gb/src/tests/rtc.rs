use crate::gb::cartridge::{RAM_BANK_SIZE, RTC_CYCLES_PER_SECOND};
use crate::gb::{GameBoy, GbModel};
use crate::persistence::rtc_footer::{RTC_FILE_LEN, RTC_FOOTER_LEN, RtcFooter};
use crate::persistence::sram_dump::SramDump;
use crate::rom::Rom;
use crate::{ReadMemory, WriteMemory};

const NOW: u64 = 1_761_400_000;
const HOUR: u64 = 60 * 60;

fn rtc_rom(cartridge_type: u8, ram_size_code: u8) -> Vec<u8> {
    let mut data = vec![0u8; 0x8000];
    data[0x0147] = cartridge_type;
    data[0x0148] = 0x00;
    data[0x0149] = ram_size_code;
    data
}

fn loaded(data: &[u8]) -> GameBoy {
    let mut gb = GameBoy::new_empty(GbModel::Dmg);
    gb.load_rom(&Rom::new(data)).expect("load");
    gb
}

fn rtc_cart() -> GameBoy {
    loaded(&rtc_rom(0x10, 0x02))
}

fn set_clock(gb: &mut GameBoy, registers: [u8; 5]) {
    gb.cartridge.write_naive(0x0000, 0x0A);
    gb.cartridge.write_naive(0x4000, 0x0C);
    gb.cartridge.write_naive(0xA000, 0x40);
    for (register, value) in (0x08..=0x0C).zip(registers) {
        gb.cartridge.write_naive(0x4000, register);
        gb.cartridge.write_naive(0xA000, value);
    }
}

fn read_clock(gb: &mut GameBoy) -> [u8; 5] {
    gb.cartridge.write_naive(0x0000, 0x0A);
    gb.cartridge.write_naive(0x6000, 0x00);
    gb.cartridge.write_naive(0x6000, 0x01);
    [0x08, 0x09, 0x0A, 0x0B, 0x0C].map(|register| {
        gb.cartridge.write_naive(0x4000, register);
        gb.cartridge.read_naive(0xA000)
    })
}

fn write_ram(gb: &mut GameBoy, bytes: &[u8]) {
    gb.cartridge.write_naive(0x0000, 0x0A);
    gb.cartridge.write_naive(0x4000, 0x00);
    for (i, byte) in bytes.iter().enumerate() {
        gb.cartridge.write_naive(0xA000 + i as u16, *byte);
    }
}

fn read_ram(gb: &mut GameBoy, len: usize) -> Vec<u8> {
    gb.cartridge.write_naive(0x0000, 0x0A);
    gb.cartridge.write_naive(0x4000, 0x00);
    (0..len)
        .map(|i| gb.cartridge.read_naive(0xA000 + i as u16))
        .collect()
}

#[test]
fn clock_and_rumble_carts_with_a_battery_get_battery_saves() {
    for cartridge_type in [0x0F, 0x10, 0x1E] {
        let mut gb = loaded(&rtc_rom(cartridge_type, 0x02));
        assert!(gb.cartridge.has_battery(), "type {cartridge_type:02X}");
        assert!(gb.poll_sram_dump(true, NOW).is_some());
    }
}

#[test]
fn a_reset_keeps_battery_ram_and_the_clock() {
    let mut gb = rtc_cart();
    write_ram(&mut gb, b"save data");
    set_clock(&mut gb, [12, 34, 5, 6, 0]);
    gb.soft_reset();
    assert_eq!(read_ram(&mut gb, 9), b"save data");
    assert_eq!(read_clock(&mut gb), [12, 34, 5, 6, 0]);
}

#[test]
fn a_reset_clears_ram_without_a_battery() {
    let mut gb = loaded(&rtc_rom(0x12, 0x02));
    assert!(!gb.cartridge.has_battery());
    write_ram(&mut gb, b"scratch");
    gb.soft_reset();
    assert_eq!(read_ram(&mut gb, 7), vec![0; 7]);
}

#[test]
fn the_clock_runs_with_emulated_time() {
    let mut gb = rtc_cart();
    set_clock(&mut gb, [58, 0, 0, 0, 0]);
    gb.run_cycles(2 * RTC_CYCLES_PER_SECOND);
    assert_eq!(read_clock(&mut gb), [0, 1, 0, 0, 0]);
}

#[test]
fn the_clock_keeps_real_time_in_double_speed() {
    let mut gb = GameBoy::new_empty(GbModel::Cgb);
    gb.load_rom(&Rom::new(&rtc_rom(0x10, 0x02))).expect("load");
    gb.cgb.double_speed = true;
    set_clock(&mut gb, [58, 0, 0, 0, 0]);
    gb.run_cycles(2 * RTC_CYCLES_PER_SECOND);
    assert_eq!(read_clock(&mut gb), [59, 0, 0, 0, 0]);
    gb.run_cycles(2 * RTC_CYCLES_PER_SECOND);
    assert_eq!(read_clock(&mut gb), [0, 1, 0, 0, 0]);
}

#[test]
fn battery_saves_end_with_the_clock_footer() {
    let mut gb = rtc_cart();
    write_ram(&mut gb, b"save data");
    set_clock(&mut gb, [12, 34, 5, 6, 0]);

    let dump = gb.poll_sram_dump(true, NOW).expect("battery");
    let data = dump.as_slice();
    assert_eq!(data.len(), RAM_BANK_SIZE + RTC_FOOTER_LEN);
    assert_eq!(&data[..9], b"save data");

    let footer = RtcFooter::decode(&data[RAM_BANK_SIZE..]).expect("footer");
    assert_eq!(footer.real.to_bytes(), [12, 34, 5, 6, 0]);
    assert_eq!(footer.latched, footer.real);
    assert_eq!(footer.timestamp, NOW);
}

#[test]
fn the_footer_carries_the_running_time_in_both_register_sets() {
    let mut gb = rtc_cart();
    set_clock(&mut gb, [1, 0, 0, 0, 0]);
    read_clock(&mut gb);
    gb.run_cycles(3 * RTC_CYCLES_PER_SECOND);

    let dump = gb.poll_sram_dump(true, NOW).expect("battery");
    let footer = RtcFooter::decode(&dump.as_slice()[RAM_BANK_SIZE..]).expect("footer");
    assert_eq!(footer.real.to_bytes(), [4, 0, 0, 0, 0]);
    assert_eq!(footer.latched, footer.real);
}

#[test]
fn a_clock_only_cart_saves_just_the_footer() {
    let mut gb = loaded(&rtc_rom(0x0F, 0x00));
    let dump = gb.poll_sram_dump(true, NOW).expect("battery");
    assert_eq!(dump.as_slice().len(), RTC_FOOTER_LEN);
}

#[test]
fn loading_a_save_catches_up_with_the_time_since_it_was_written() {
    let mut gb = rtc_cart();
    write_ram(&mut gb, b"kept");
    set_clock(&mut gb, [0, 30, 22, 0x10, 0]);
    let dump = gb.poll_sram_dump(true, NOW).expect("battery");

    let mut later = rtc_cart();
    later.put_sram_dump(SramDump::from_slice(dump.as_slice()), NOW + 3 * HOUR);

    assert_eq!(read_clock(&mut later), [0, 30, 1, 0x11, 0]);
    assert_eq!(read_ram(&mut later, 4), b"kept");
}

#[test]
fn a_halted_clock_does_not_catch_up() {
    let mut gb = rtc_cart();
    set_clock(&mut gb, [1, 2, 3, 4, 0x40]);
    let dump = gb.poll_sram_dump(true, NOW).expect("battery");

    let mut later = rtc_cart();
    later.put_sram_dump(SramDump::from_slice(dump.as_slice()), NOW + 3 * HOUR);
    assert_eq!(read_clock(&mut later), [1, 2, 3, 4, 0x40]);
}

#[test]
fn a_save_without_a_footer_still_loads() {
    let mut old = vec![0u8; RAM_BANK_SIZE];
    old[..3].copy_from_slice(b"old");

    let mut gb = rtc_cart();
    gb.put_sram_dump(SramDump::from_slice(&old), NOW);

    assert_eq!(read_ram(&mut gb, 3), b"old");
    assert_eq!(read_clock(&mut gb), [0, 0, 0, 0, 0]);
}

#[test]
fn carts_without_a_clock_save_only_their_ram() {
    let mut gb = loaded(&rtc_rom(0x13, 0x02));
    let dump = gb.poll_sram_dump(true, NOW).expect("battery");
    assert_eq!(dump.as_slice().len(), RAM_BANK_SIZE);
}

#[test]
fn snapshots_keep_the_clock_and_catch_up_on_load() {
    let mut gb = rtc_cart();
    set_clock(&mut gb, [10, 0, 0, 0, 0]);
    let rom = rtc_rom(0x10, 0x02);

    let state = gb.dump_full(NOW).expect("dump");

    let mut same_time = GameBoy::from_dump(&state, &Rom::new(&rom), NOW).expect("restore");
    assert_eq!(read_clock(&mut same_time), [10, 0, 0, 0, 0]);

    let mut later = GameBoy::from_dump(&state, &Rom::new(&rom), NOW + 65).expect("restore");
    assert_eq!(read_clock(&mut later), [15, 1, 0, 0, 0]);
}

#[test]
fn snapshots_and_exports_do_not_swallow_a_pending_battery_write() {
    let mut gb = rtc_cart();
    write_ram(&mut gb, b"x");
    gb.dump_full(NOW).expect("dump");
    gb.poll_sram_dump(true, NOW).expect("export");
    assert!(gb.poll_sram_dump(false, NOW).is_some());
    assert!(gb.poll_sram_dump(false, NOW).is_none());
}

#[test]
fn the_sameboy_fixture_imports_with_its_clock() {
    let data = include_bytes!("../../tests/fixtures/sameboy-rtc.sav");
    assert_eq!(data.len(), RAM_BANK_SIZE + RTC_FOOTER_LEN);

    let footer = RtcFooter::decode(&data[RAM_BANK_SIZE..]).expect("footer");
    let mut gb = rtc_cart();
    gb.put_sram_dump(SramDump::from_slice(data), footer.timestamp);

    assert_eq!(read_clock(&mut gb), [42, 17, 9, 0x2C, 0x41]);
    assert_eq!(read_ram(&mut gb, 8), b"CITRINE!");
}

#[test]
fn the_sameboy_libretro_pair_imports_with_its_clock() {
    let srm = include_bytes!("../../tests/fixtures/sameboy-libretro.srm");
    let rtc = include_bytes!("../../tests/fixtures/sameboy-libretro.rtc");
    assert_eq!(rtc.len(), RTC_FILE_LEN);

    let footer = RtcFooter::decode_rtc_file(rtc).expect("rtc file");
    let mut gb = rtc_cart();
    gb.put_sram_dump(SramDump::from_slice(srm), footer.timestamp);
    assert!(
        gb.cartridge
            .put_rtc_footer(footer, footer.timestamp + 2 * HOUR)
    );

    assert_eq!(read_clock(&mut gb), [42, 17, 9, 0x2C, 0x41]);
    assert_eq!(read_ram(&mut gb, 8), b"CITRINE!");
}

#[test]
fn a_running_sameboy_libretro_clock_catches_up_into_the_next_day() {
    let rtc = include_bytes!("../../tests/fixtures/sameboy-libretro-rtc3test.rtc");
    let footer = RtcFooter::decode_rtc_file(rtc).expect("rtc file");
    assert_eq!(footer.real.to_bytes(), [59, 59, 23, 0xFF, 0x00]);

    let mut gb = loaded(&rtc_rom(0x0F, 0x00));
    assert!(gb.cartridge.put_rtc_footer(footer, footer.timestamp + 1));
    assert_eq!(read_clock(&mut gb), [0, 0, 0, 0x00, 0x01]);
}

#[test]
fn the_rtc_file_export_matches_what_sameboy_libretro_writes() {
    let rtc = include_bytes!("../../tests/fixtures/sameboy-libretro.rtc");
    let footer = RtcFooter::decode_rtc_file(rtc).expect("rtc file");

    let mut gb = rtc_cart();
    gb.cartridge.put_rtc_footer(footer, footer.timestamp);
    let exported = gb
        .cartridge
        .rtc_footer(footer.timestamp)
        .expect("clock")
        .encode_rtc_file();
    assert_eq!(exported[..24], rtc[..24]);
    assert_eq!(RtcFooter::decode_rtc_file(&exported), Some(footer));
}

#[test]
fn a_cartridge_without_a_clock_refuses_an_rtc_file() {
    let rtc = include_bytes!("../../tests/fixtures/sameboy-libretro.rtc");
    let footer = RtcFooter::decode_rtc_file(rtc).expect("rtc file");
    let mut gb = loaded(&rtc_rom(0x03, 0x02));
    assert!(!gb.cartridge.put_rtc_footer(footer, footer.timestamp));
    assert!(gb.cartridge.rtc_footer(footer.timestamp).is_none());
}

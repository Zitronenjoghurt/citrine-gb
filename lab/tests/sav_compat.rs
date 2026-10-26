use citrine_gb::gb::{GameBoy, GbModel};
use citrine_gb::persistence::rtc_footer::{RTC_FOOTER_LEN, RtcFooter};
use citrine_gb::persistence::sram_dump::SramDump;
use citrine_gb::rom::Rom;
use citrine_gb::{ReadMemory, WriteMemory};
use citrine_lab::emulator::FrameEmulator;
use citrine_lab::emulators::SameBoyEmulator;
use std::path::PathBuf;

const RAM_LEN: usize = 0x2000;
const HALT: u8 = 0x40;
const FIXTURE_RAM: &[u8; 8] = b"CITRINE!";
const FIXTURE_CLOCK: [u8; 5] = [42, 17, 9, 0x2C, 0x41];

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../lib/tests/fixtures/sameboy-rtc.sav")
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after 1970")
        .as_secs()
}

fn clock_rom(ram: &[u8], clock: [u8; 5]) -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x0100..0x0104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]);
    rom[0x0147] = 0x10;
    rom[0x0148] = 0x00;
    rom[0x0149] = 0x02;

    let mut code = Vec::new();
    let store = |code: &mut Vec<u8>, addr: u16, value: u8| {
        code.extend_from_slice(&[0x3E, value, 0xEA, addr as u8, (addr >> 8) as u8]);
    };
    store(&mut code, 0x0000, 0x0A);
    store(&mut code, 0x4000, 0x00);
    for (i, byte) in ram.iter().enumerate() {
        store(&mut code, 0xA000 + i as u16, *byte);
    }
    store(&mut code, 0x4000, 0x0C);
    store(&mut code, 0xA000, HALT);
    for (register, value) in (0x08..=0x0C).zip(clock) {
        store(&mut code, 0x4000, register);
        store(&mut code, 0xA000, value);
    }
    store(&mut code, 0x6000, 0x00);
    store(&mut code, 0x6000, 0x01);
    store(&mut code, 0x0000, 0x00);
    code.extend_from_slice(&[0x18, 0xFE]);
    rom[0x0150..0x0150 + code.len()].copy_from_slice(&code);
    rom
}

fn sameboy_battery(rom: &[u8]) -> Vec<u8> {
    let mut sameboy = SameBoyEmulator::new();
    sameboy.load(rom, None, GbModel::Dmg).expect("load");
    let mut frames = 0;
    while frames < 10 {
        if sameboy.step() {
            frames += 1;
        }
    }
    sameboy.save_battery()
}

fn sameboy_reload(rom: &[u8], battery: &[u8]) -> Vec<u8> {
    let mut sameboy = SameBoyEmulator::new();
    sameboy.load(rom, None, GbModel::Dmg).expect("load");
    sameboy.load_battery(battery);
    sameboy.save_battery()
}

fn citrine(rom: &[u8]) -> GameBoy {
    let mut gb = GameBoy::new_empty(GbModel::Dmg);
    gb.load_rom(&Rom::new(rom)).expect("load");
    gb
}

fn citrine_set(gb: &mut GameBoy, ram: &[u8], clock: [u8; 5]) {
    gb.cartridge.write_naive(0x0000, 0x0A);
    gb.cartridge.write_naive(0x4000, 0x00);
    for (i, byte) in ram.iter().enumerate() {
        gb.cartridge.write_naive(0xA000 + i as u16, *byte);
    }
    gb.cartridge.write_naive(0x4000, 0x0C);
    gb.cartridge.write_naive(0xA000, HALT);
    for (register, value) in (0x08..=0x0C).zip(clock) {
        gb.cartridge.write_naive(0x4000, register);
        gb.cartridge.write_naive(0xA000, value);
    }
}

fn citrine_clock(gb: &mut GameBoy) -> [u8; 5] {
    gb.cartridge.write_naive(0x0000, 0x0A);
    gb.cartridge.write_naive(0x6000, 0x00);
    gb.cartridge.write_naive(0x6000, 0x01);
    [0x08, 0x09, 0x0A, 0x0B, 0x0C].map(|register| {
        gb.cartridge.write_naive(0x4000, register);
        gb.cartridge.read_naive(0xA000)
    })
}

fn footer_of(battery: &[u8]) -> RtcFooter {
    assert_eq!(battery.len(), RAM_LEN + RTC_FOOTER_LEN);
    RtcFooter::decode(&battery[RAM_LEN..]).expect("footer")
}

fn export(gb: &mut GameBoy, at: u64) -> Vec<u8> {
    gb.poll_sram_dump(true, at)
        .expect("battery")
        .as_slice()
        .to_vec()
}

#[test]
#[ignore = "rewrites lib/tests/fixtures/sameboy-rtc.sav"]
fn write_sameboy_fixture() {
    let battery = sameboy_battery(&clock_rom(FIXTURE_RAM, FIXTURE_CLOCK));
    let path = fixture_path();
    std::fs::create_dir_all(path.parent().expect("fixture folder")).expect("fixture folder");
    std::fs::write(path, battery).expect("write fixture");
}

#[test]
fn sameboy_still_writes_the_committed_fixture() {
    let fixture = std::fs::read(fixture_path()).expect("fixture");
    let fresh = sameboy_battery(&clock_rom(FIXTURE_RAM, FIXTURE_CLOCK));
    assert_eq!(fresh.len(), fixture.len());
    assert_eq!(fresh[..fresh.len() - 8], fixture[..fixture.len() - 8]);
    assert_eq!(footer_of(&fixture).real.to_bytes(), FIXTURE_CLOCK);
}

#[test]
fn a_sameboy_save_with_a_running_clock_imports_and_catches_up() {
    let rom = clock_rom(b"RUNNING", [10, 20, 3, 5, 0]);
    let battery = sameboy_battery(&rom);
    let footer = footer_of(&battery);

    let mut gb = citrine(&rom);
    gb.put_sram_dump(SramDump::from_slice(&battery), footer.timestamp + 2 * 3600);

    let mut expected = footer.real.to_bytes();
    expected[2] += 2;
    assert_eq!(citrine_clock(&mut gb), expected);
    gb.cartridge.write_naive(0x4000, 0x00);
    assert_eq!(gb.cartridge.read_naive(0xA000), b'R');
}

#[test]
fn a_citrine_export_loads_in_sameboy_and_catches_up() {
    let rom = clock_rom(b"", [0; 5]);
    let mut gb = citrine(&rom);
    citrine_set(&mut gb, b"EXPORTED", [5, 10, 20, 100, 0]);

    let before = now();
    let reloaded = sameboy_reload(&rom, &export(&mut gb, before - 3600));
    let footer = footer_of(&reloaded);

    let real = footer.real.to_bytes();
    let late = (footer.timestamp - before) as u8;
    assert!((5..=5 + late).contains(&real[0]), "seconds {}", real[0]);
    assert_eq!(real[1..], [10, 21, 100, 0]);
    assert_eq!(&reloaded[..8], b"EXPORTED");
}

#[test]
fn a_halted_clock_and_its_latch_survive_sameboy_exactly() {
    let rom = clock_rom(b"", [0; 5]);
    let mut gb = citrine(&rom);
    citrine_set(&mut gb, b"", [1, 2, 3, 4, HALT]);
    assert_eq!(citrine_clock(&mut gb), [1, 2, 3, 4, HALT]);
    gb.cartridge.write_naive(0x4000, 0x0C);
    gb.cartridge.write_naive(0xA000, HALT | 0x01);

    let reloaded = sameboy_reload(&rom, &export(&mut gb, now() - 3600));
    let footer = footer_of(&reloaded);
    assert_eq!(footer.real.to_bytes(), [1, 2, 3, 4, HALT | 0x01]);
    assert_eq!(footer.latched.to_bytes(), [1, 2, 3, 4, HALT | 0x01]);
}

#[test]
fn the_day_carry_matches_sameboy_after_an_overflow() {
    let rom = clock_rom(b"", [0; 5]);
    let mut gb = citrine(&rom);
    citrine_set(&mut gb, b"", [0, 30, 23, 0xFF, 0x01]);

    let before = now();
    let exported = export(&mut gb, before - 3600);
    let reloaded = sameboy_reload(&rom, &exported);
    let footer = footer_of(&reloaded);
    let real = footer.real.to_bytes();
    let late = (footer.timestamp - before) as u8;
    assert!(real[0] <= late, "seconds {}", real[0]);
    assert_eq!(real[1..], [30, 0, 0x00, 0x80]);

    let mut back = citrine(&rom);
    back.put_sram_dump(SramDump::from_slice(&exported), before);
    assert_eq!(citrine_clock(&mut back), [0, 30, 0, 0x00, 0x80]);
}

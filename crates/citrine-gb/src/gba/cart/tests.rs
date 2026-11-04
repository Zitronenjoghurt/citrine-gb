use super::{DateTime, Devices};
use crate::gba::bus::Bus;
use crate::gba::machine::Gba;

const DATA: u32 = 0x0800_00C4;
const DIRECTION: u32 = 0x0800_00C6;
const CONTROL: u32 = 0x0800_00C8;
const SCK: u16 = 1;
const SIO: u16 = 2;
const CS: u16 = 4;
const RESET: u8 = 0x60;
const STATUS: u8 = 0x62;
const DATE_TIME: u8 = 0x64;
const TIME: u8 = 0x66;
const READ: u8 = 1;
const SECOND: u32 = 16_777_216;
const LOOP: u32 = 0xEAFF_FFFE;

fn rom(code: &[u8; 4]) -> Vec<u8> {
    let mut rom = vec![0; 0x200];
    rom[..4].copy_from_slice(&LOOP.to_le_bytes());
    rom[0xAC..0xB0].copy_from_slice(code);
    rom
}

fn send(bus: &mut Bus, value: u8, msb_first: bool) {
    for i in 0..8 {
        let bit = if msb_first {
            value >> (7 - i)
        } else {
            value >> i
        } & 1;
        let sio = u16::from(bit) << 1;
        bus.write16(DATA, sio | CS);
        bus.write16(DATA, sio | SCK | CS);
    }
}

fn receive(bus: &mut Bus) -> u8 {
    let mut value = 0;
    for i in 0..8 {
        bus.write16(DATA, CS);
        bus.write16(DATA, CS);
        bus.write16(DATA, SCK | CS);
        value |= (((bus.read16(DATA) & SIO) >> 1) as u8) << i;
    }
    value
}

fn rtc_write(bus: &mut Bus, command: u8, bytes: &[u8]) {
    bus.write16(CONTROL, 1);
    bus.write16(DIRECTION, 7);
    bus.write16(DATA, SCK);
    bus.write16(DATA, SCK | CS);
    send(bus, command, true);
    for &byte in bytes {
        send(bus, byte, false);
    }
    bus.write16(DATA, SCK);
}

fn rtc_read<const N: usize>(bus: &mut Bus, command: u8) -> [u8; N] {
    bus.write16(CONTROL, 1);
    bus.write16(DIRECTION, 7);
    bus.write16(DATA, SCK);
    bus.write16(DATA, SCK | CS);
    send(bus, command | READ, true);
    bus.write16(DIRECTION, 5);
    let bytes = std::array::from_fn(|_| receive(bus));
    bus.write16(DATA, SCK);
    bus.write16(DIRECTION, 7);
    bytes
}

fn clock_bus() -> Bus {
    Bus::new(&rom(b"BPEE"))
}

#[test]
fn the_clock_reads_back_the_date_written_through_the_port() {
    let mut bus = clock_bus();
    let date = [0x24, 0x02, 0x29, 0x04, 0x93, 0x45, 0x30];
    rtc_write(&mut bus, DATE_TIME, &date);
    assert_eq!(rtc_read::<7>(&mut bus, DATE_TIME), date);
    assert_eq!(rtc_read::<3>(&mut bus, TIME), [0x93, 0x45, 0x30]);
    assert_eq!(
        bus.cart().clock(),
        DateTime {
            year: 24,
            month: 2,
            day: 29,
            hour: 13,
            minute: 45,
            second: 30,
        }
    );
}

#[test]
fn a_time_write_keeps_the_date() {
    let mut bus = clock_bus();
    rtc_write(
        &mut bus,
        DATE_TIME,
        &[0x05, 0x11, 0x02, 0x03, 0x08, 0x00, 0x00],
    );
    rtc_write(&mut bus, TIME, &[0x21, 0x15, 0x09]);
    assert_eq!(
        rtc_read::<7>(&mut bus, DATE_TIME),
        [0x05, 0x11, 0x02, 0x03, 0xA1, 0x15, 0x09]
    );
}

#[test]
fn the_clock_rolls_over_into_a_leap_day() {
    let mut bus = clock_bus();
    rtc_write(
        &mut bus,
        DATE_TIME,
        &[0x24, 0x02, 0x28, 0x03, 0x23, 0x59, 0x59],
    );
    bus.step(SECOND);
    assert_eq!(
        rtc_read::<7>(&mut bus, DATE_TIME),
        [0x24, 0x02, 0x29, 0x04, 0x00, 0x00, 0x00]
    );
}

#[test]
fn the_clock_rolls_over_from_2099_to_2000() {
    let mut bus = clock_bus();
    rtc_write(
        &mut bus,
        DATE_TIME,
        &[0x99, 0x12, 0x31, 0x06, 0x23, 0x59, 0x59],
    );
    bus.step(SECOND - 1);
    assert_eq!(rtc_read::<3>(&mut bus, TIME), [0xA3, 0x59, 0x59]);
    bus.step(1);
    let date = rtc_read::<7>(&mut bus, DATE_TIME);
    assert_eq!(date[..3], [0x00, 0x01, 0x01]);
    assert_eq!(date[4..], [0x00, 0x00, 0x00]);
}

#[test]
fn the_clock_starts_from_the_host_time() {
    let mut bus = clock_bus();
    bus.cart_mut().set_clock(1_761_955_200);
    assert_eq!(
        rtc_read::<7>(&mut bus, DATE_TIME),
        [0x25, 0x11, 0x01, 0x06, 0x00, 0x00, 0x00]
    );
}

#[test]
fn the_status_register_keeps_its_writable_bits_and_switches_to_12_hours() {
    let mut bus = clock_bus();
    assert_eq!(rtc_read::<1>(&mut bus, STATUS), [0x40]);
    rtc_write(&mut bus, STATUS, &[0xFF]);
    assert_eq!(rtc_read::<1>(&mut bus, STATUS), [0x6A]);
    rtc_write(&mut bus, STATUS, &[0x00]);
    rtc_write(
        &mut bus,
        DATE_TIME,
        &[0x24, 0x01, 0x01, 0x01, 0x81, 0x00, 0x00],
    );
    assert_eq!(rtc_read::<3>(&mut bus, TIME), [0x81, 0x00, 0x00]);
    assert_eq!(bus.cart().clock().hour, 13);
}

#[test]
fn a_reset_command_clears_the_date_and_the_status() {
    let mut bus = clock_bus();
    rtc_write(
        &mut bus,
        DATE_TIME,
        &[0x24, 0x05, 0x06, 0x01, 0x12, 0x34, 0x56],
    );
    rtc_write(&mut bus, RESET, &[]);
    assert_eq!(rtc_read::<1>(&mut bus, STATUS), [0x00]);
    assert_eq!(
        rtc_read::<7>(&mut bus, DATE_TIME),
        [0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00]
    );
}

#[test]
fn unused_registers_read_ff() {
    let mut bus = clock_bus();
    assert_eq!(rtc_read::<1>(&mut bus, 0x68), [0xFF]);
}

#[test]
fn the_port_reads_as_rom_until_reads_are_enabled() {
    let mut data = rom(b"BPEE");
    data[0xC4..0xCA].copy_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
    let mut bus = Bus::new(&data);
    bus.write16(DIRECTION, 7);
    assert_eq!(bus.read32(DATA), 0x4433_2211);
    bus.write16(CONTROL, 1);
    assert_eq!(bus.read16(DIRECTION), 7);
    assert_eq!(bus.read16(CONTROL), 1);
    assert_eq!(bus.read16(0x0800_00CA), 0);
    bus.write8(DIRECTION, 0);
    assert_eq!(bus.read16(DIRECTION), 7);
}

#[test]
fn devices_come_from_the_game_code() {
    let devices = |code| Bus::new(&rom(code)).cart().devices();
    assert!(devices(b"AXVE").rtc);
    assert!(devices(b"AXPD").rtc);
    let boktai = devices(b"U3IJ");
    assert!(boktai.rtc && boktai.solar && !boktai.gyro);
    let twisted = devices(b"RZWE");
    assert!(twisted.gyro && twisted.rumble && !twisted.rtc);
    assert!(devices(b"V49E").rumble);
    assert!(devices(b"KYGP").tilt && !devices(b"KYGP").uses_gpio());
    assert!(devices(b"KHPJ").tilt);
    assert_eq!(devices(b"AGBE"), Devices::default());
}

#[test]
fn an_unknown_game_gets_a_clock_at_its_first_port_write() {
    let mut bus = Bus::new(&rom(b"ZZZE"));
    assert!(!bus.cart().devices().rtc);
    bus.write8(CONTROL, 1);
    assert!(!bus.cart().devices().rtc);
    rtc_write(
        &mut bus,
        DATE_TIME,
        &[0x10, 0x10, 0x10, 0x00, 0x10, 0x10, 0x10],
    );
    assert!(bus.cart().devices().rtc);
    assert_eq!(rtc_read::<3>(&mut bus, TIME), [0x10, 0x10, 0x10]);
}

fn read_gyro(bus: &mut Bus) -> u16 {
    bus.write16(CONTROL, 1);
    bus.write16(DIRECTION, 0x0B);
    bus.write16(DATA, 3);
    bus.write16(DATA, 2);
    let mut value = 0;
    for _ in 0..16 {
        value = (value << 1) | ((bus.read16(DATA) >> 2) & 1);
        bus.write16(DATA, 0);
        bus.write16(DATA, 2);
    }
    value & 0x0FFF
}

#[test]
fn the_gyro_reads_as_wario_ware_reads_it() {
    let mut bus = Bus::new(&rom(b"RZWE"));
    assert_eq!(read_gyro(&mut bus), 0x6C0);
    bus.cart_mut().set_turn(-1.0);
    assert_eq!(read_gyro(&mut bus), 0x354);
    bus.cart_mut().set_turn(1.0);
    assert_eq!(read_gyro(&mut bus), 0x9E3);
    bus.cart_mut().set_turn(0.5);
    assert_eq!(read_gyro(&mut bus), 0x852);
}

fn read_solar(bus: &mut Bus) -> u16 {
    bus.write16(CONTROL, 1);
    bus.write16(DIRECTION, 7);
    bus.write16(DATA, 2);
    bus.write16(DATA, 0);
    let mut count = 0;
    while count < 0x100 {
        bus.write16(DATA, 1);
        bus.write16(DATA, 0);
        if bus.read16(DATA) & 8 != 0 {
            break;
        }
        count += 1;
    }
    count
}

#[test]
fn the_solar_sensor_reads_as_boktai_reads_it() {
    let mut bus = Bus::new(&rom(b"U3IE"));
    assert_eq!(read_solar(&mut bus), 0xE8);
    bus.cart_mut().set_sunlight(1.0);
    assert_eq!(read_solar(&mut bus), 0x50);
    bus.cart_mut().set_sunlight(0.5);
    assert_eq!(read_solar(&mut bus), 0x9C);
}

fn read_tilt(bus: &mut Bus) -> (u16, u16, bool) {
    bus.write8(0x0E00_8000, 0x55);
    bus.write8(0x0E00_8100, 0xAA);
    let high = bus.read8(0x0E00_8300);
    let x = (u16::from(high & 0x0F) << 8) | u16::from(bus.read8(0x0E00_8200));
    let y = (u16::from(bus.read8(0x0E00_8500) & 0x0F) << 8) | u16::from(bus.read8(0x0E00_8400));
    (x, y, high & 0x80 != 0)
}

#[test]
fn the_tilt_registers_hold_x_and_y_from_the_last_sample() {
    let mut bus = Bus::new(&rom(b"KYGE"));
    assert_eq!(read_tilt(&mut bus), (0x392, 0x3A0, true));
    bus.cart_mut().set_tilt(-1.0, 1.0);
    assert_eq!(read_tilt(&mut bus), (0x2AF, 0x480, true));
    bus.cart_mut().set_tilt(1.0, -1.0);
    bus.write8(0x0E00_8100, 0xAA);
    assert_eq!(bus.read8(0x0E00_8200), 0xAF);
    assert_eq!(read_tilt(&mut bus), (0x477, 0x2C3, true));
}

#[test]
fn the_tilt_window_is_backup_memory_without_a_sensor() {
    let mut data = rom(b"AGBE");
    data.extend_from_slice(b"SRAM_V113");
    let mut bus = Bus::new(&data);
    bus.write8(0x0E00_8200, 0x42);
    assert_eq!(bus.read8(0x0E00_8200), 0x42);
}

fn rumble_game() -> Gba {
    Gba::new(&rom(b"V49E"))
}

#[test]
fn rumble_is_on_for_each_frame_the_motor_ran() {
    let mut gba = rumble_game();
    gba.run_frame();
    assert!(!gba.rumble());
    gba.bus_mut().write16(DIRECTION, 8);
    gba.bus_mut().write16(DATA, 8);
    gba.run_frame();
    assert!(gba.rumble());
    gba.bus_mut().write16(DATA, 0);
    gba.run_frame();
    assert!(gba.rumble());
    gba.run_frame();
    assert!(!gba.rumble());
    gba.bus_mut().write16(DATA, 8);
    gba.bus_mut().write16(DIRECTION, 0);
    gba.run_frame();
    gba.run_frame();
    assert!(!gba.rumble());
}

#[test]
fn a_soft_reset_keeps_the_backup_the_clock_and_the_port() {
    let mut data = rom(b"ZZZE");
    data.extend_from_slice(b"SRAM_V113");
    let mut gba = Gba::new(&data);
    gba.bus_mut().write8(0x0E00_0010, 0x42);
    rtc_write(
        gba.bus_mut(),
        DATE_TIME,
        &[0x25, 0x11, 0x02, 0x00, 0x12, 0x43, 0x00],
    );
    gba.bus_mut().write16(DIRECTION, 5);
    gba.soft_reset();
    assert_eq!(gba.bus().read8(0x0E00_0010), 0x42);
    assert!(gba.cart_devices().rtc);
    assert_eq!(gba.bus().read16(CONTROL), 1);
    assert_eq!(gba.bus().read16(DIRECTION), 5);
    assert_eq!(
        rtc_read::<7>(gba.bus_mut(), DATE_TIME),
        [0x25, 0x11, 0x02, 0x00, 0x92, 0x43, 0x00]
    );
}

#[cfg(feature = "persistence")]
mod battery {
    use super::*;
    use crate::console::Console;

    const NOW: u64 = 1_761_955_200;

    fn emerald() -> Gba {
        let mut data = rom(b"BPEE");
        data.extend_from_slice(b"FLASH1M_V103");
        data.resize(0x400, 0);
        Gba::new(&data)
    }

    #[test]
    fn a_battery_save_keeps_the_clock_after_the_chip() {
        let mut gba = emerald();
        assert!(gba.has_clock());
        rtc_write(
            gba.bus_mut(),
            DATE_TIME,
            &[0x04, 0x09, 0x17, 0x05, 0x10, 0x00, 0x00],
        );
        let save = gba.battery(true, NOW).expect("save");
        assert_eq!(save.clock, None);
        assert!(save.seiko.is_some());
        let file = save.to_file();
        assert_eq!(file.len(), 0x2_0000 + 16);
        let mut later = emerald();
        let loaded = later.battery_from_file(&file);
        assert_eq!(loaded.ram, save.ram);
        assert_eq!(loaded.seiko, save.seiko);
        later.put_battery(loaded, NOW + 90 * 60);
        assert_eq!(
            rtc_read::<7>(later.bus_mut(), DATE_TIME),
            [0x04, 0x09, 0x17, 0x05, 0x11, 0x30, 0x00]
        );
    }

    #[test]
    fn the_power_off_flag_clears_when_read() {
        let mut gba = emerald();
        let mut save = gba.battery(true, NOW).expect("save");
        let Some(footer) = &mut save.seiko else {
            panic!("clock");
        };
        footer.status = 0xC0;
        gba.put_battery(save, NOW);
        assert_eq!(rtc_read::<1>(gba.bus_mut(), STATUS), [0xC0]);
        assert_eq!(rtc_read::<1>(gba.bus_mut(), STATUS), [0x40]);
    }

    #[test]
    fn a_game_without_a_clock_saves_the_plain_chip() {
        let mut data = rom(b"AGBE");
        data.extend_from_slice(b"SRAM_V113");
        data.resize(0x400, 0);
        let mut gba = Gba::new(&data);
        assert!(!gba.has_clock());
        let save = gba.battery(true, NOW).expect("save");
        assert_eq!(save.seiko, None);
        assert_eq!(save.to_file().len(), 0x8000);
    }
}

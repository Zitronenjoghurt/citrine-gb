use crate::console::Console;
use crate::gb::{GameBoy, GbModel};
use crate::rom::Rom;
use crate::{ReadMemory, WriteMemory};

const NOW: u64 = 1_761_400_000;

fn rom(cartridge_type: u8, rom_size_code: u8, ram_size_code: u8) -> Vec<u8> {
    let mut data = vec![0u8; 0x8000 << rom_size_code];
    data[0x0147] = cartridge_type;
    data[0x0148] = rom_size_code;
    data[0x0149] = ram_size_code;
    for (bank, chunk) in data.chunks_mut(0x4000).enumerate() {
        chunk[0x2000] = bank as u8;
    }
    data
}

fn loaded(data: &[u8]) -> GameBoy {
    let mut gb = GameBoy::new_empty(GbModel::Dmg);
    gb.load_rom(&Rom::new(data)).expect("load");
    gb
}

#[test]
fn mbc30_reaches_rom_banks_past_127_and_eight_ram_banks() {
    let mut gb = loaded(&rom(0x10, 0x07, 0x05));
    gb.cartridge.write_naive(0x2000, 0xC8);
    assert_eq!(gb.cartridge.read_naive(0x6000), 0xC8);
    gb.cartridge.write_naive(0x0000, 0x0A);
    for bank in 0..8 {
        gb.cartridge.write_naive(0x4000, bank);
        gb.cartridge.write_naive(0xA000, 0x40 + bank);
    }
    for bank in 0..8 {
        gb.cartridge.write_naive(0x4000, bank);
        assert_eq!(gb.cartridge.read_naive(0xA000), 0x40 + bank);
    }
}

#[test]
fn rumble_reaches_the_machine() {
    let mut gb = loaded(&rom(0x1C, 0x01, 0x00));
    assert!(gb.has_rumble());
    gb.cartridge.write_naive(0x4000, 0x08);
    (0..100).for_each(|_| gb.cartridge.cycle());
    assert_eq!(gb.take_rumble(), 1.0);
    assert!(!loaded(&rom(0x19, 0x01, 0x00)).has_rumble());
}

#[test]
fn tilt_reaches_the_mbc7_registers() {
    let mut gb = loaded(&rom(0x22, 0x01, 0x00));
    assert!(gb.has_tilt());
    gb.set_tilt(0.0, 1.0);
    gb.cartridge.write_naive(0x0000, 0x0A);
    gb.cartridge.write_naive(0x4000, 0x40);
    gb.cartridge.write_naive(0xA000, 0x55);
    gb.cartridge.write_naive(0xA010, 0xAA);
    let y = u16::from_le_bytes([
        gb.cartridge.read_naive(0xA040),
        gb.cartridge.read_naive(0xA050),
    ]);
    assert_eq!(y, 0x81D0 + 0x70);
}

#[test]
fn mbc7_eeprom_is_the_whole_battery_file() {
    let mut gb = loaded(&rom(0x22, 0x01, 0x00));
    let save = gb.battery(true, NOW).expect("battery");
    assert_eq!(save.to_file(), vec![0xFF; 256]);
    let mut file = vec![0xFF; 256];
    file[0] = 0x12;
    let mut other = loaded(&rom(0x22, 0x01, 0x00));
    let save = other.battery_from_file(&file);
    other.put_battery(save, NOW);
    assert_eq!(other.battery(true, NOW).unwrap().to_file(), file);
}

#[test]
fn huc1_ir_reaches_the_machine() {
    let mut gb = loaded(&rom(0xFF, 0x01, 0x03));
    assert!(gb.cartridge.has_ir());
    gb.cartridge.write_naive(0x0000, 0x0E);
    gb.cartridge.set_ir_light(true);
    assert_eq!(gb.cartridge.read_naive(0xA000), 0xC1);
    gb.cartridge.write_naive(0xA000, 0x01);
    assert!(gb.cartridge.ir_led());
}

#[test]
fn huc3_saves_ram_then_a_17_byte_clock() {
    let mut gb = loaded(&rom(0xFE, 0x01, 0x03));
    gb.cartridge.write_naive(0x0000, 0x0A);
    gb.cartridge.write_naive(0xA000, 0x5A);
    let file = gb.battery(true, NOW).unwrap().to_file();
    assert_eq!(file.len(), 4 * 0x2000 + 17);
    assert_eq!(file[0], 0x5A);
    assert_eq!(&file[4 * 0x2000..4 * 0x2000 + 8], &NOW.to_le_bytes());

    let mut clock = file.clone();
    clock[4 * 0x2000 + 8..4 * 0x2000 + 10].copy_from_slice(&100u16.to_le_bytes());
    let mut other = loaded(&rom(0xFE, 0x01, 0x03));
    let save = other.battery_from_file(&clock);
    other.put_battery(save, NOW + 600);
    let back = other.battery(true, NOW + 600).unwrap().to_file();
    assert_eq!(back[0], 0x5A);
    assert_eq!(
        &back[4 * 0x2000 + 8..4 * 0x2000 + 10],
        &110u16.to_le_bytes()
    );
}

#[test]
fn camera_frames_reach_ram_bank_zero() {
    use crate::gb::cartridge::CAMERA_PIXELS;
    let mut gb = loaded(&rom(0xFC, 0x01, 0x04));
    assert!(gb.has_camera());
    gb.set_camera_frame(&vec![0; CAMERA_PIXELS]);
    gb.cartridge.write_naive(0x4000, 0x10);
    gb.cartridge.write_naive(0xA002, 0x03);
    for cell in 0..16u16 {
        for (i, threshold) in [0x80u8, 0x90, 0xA0].into_iter().enumerate() {
            gb.cartridge
                .write_naive(0xA006 + cell * 3 + i as u16, threshold);
        }
    }
    gb.cartridge.write_naive(0xA000, 0x03);
    while gb.cartridge.read_naive(0xA000) & 1 != 0 {
        gb.cartridge.cycle();
    }
    gb.cartridge.write_naive(0x4000, 0x00);
    assert_eq!(gb.cartridge.read_naive(0xA100), 0xFF);
    assert_eq!(gb.cartridge.read_naive(0xA100 + 0xDFF), 0xFF);
    assert_eq!(gb.cartridge.read_naive(0xA100 + 0xE00), 0x00);
}

fn exchange(gb: &mut GameBoy, byte: u8) -> u8 {
    gb.serial.sb = byte;
    gb.serial.write_sc(0x81, false, &mut gb.ic);
    for _ in 0..16 {
        gb.serial.div_changed(0x80, 0x00, false, &mut gb.ic);
    }
    assert!(!gb.serial.transfer_active());
    gb.serial.sb
}

fn packet(command: u8, data: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x88, 0x33, command, 0x00];
    bytes.extend_from_slice(&(data.len() as u16).to_le_bytes());
    bytes.extend_from_slice(data);
    let sum = bytes[2..]
        .iter()
        .fold(0u16, |sum, &byte| sum.wrapping_add(byte as u16));
    bytes.extend_from_slice(&sum.to_le_bytes());
    bytes.extend_from_slice(&[0x00, 0x00]);
    bytes
}

#[test]
fn a_printer_on_the_serial_port_answers_and_prints() {
    let mut gb = loaded(&rom(0x00, 0x00, 0x00));
    assert_eq!(exchange(&mut gb, 0x88), 0xFF);
    gb.attach_printer(true);
    let replies: Vec<u8> = packet(0x0F, &[])
        .into_iter()
        .map(|byte| exchange(&mut gb, byte))
        .collect();
    assert_eq!(&replies[replies.len() - 2..], &[0x81, 0x00]);
    for byte in packet(0x04, &[0x00; 640]) {
        exchange(&mut gb, byte);
    }
    for byte in packet(0x02, &[1, 0x00, 0xE4, 0x40]) {
        exchange(&mut gb, byte);
    }
    let prints = gb.take_prints();
    assert_eq!(prints.len(), 1);
    assert_eq!((prints[0].width, prints[0].height), (160, 16));
    assert!(prints[0].pixels.iter().all(|&p| p == 255));
    gb.soft_reset();
    assert!(gb.printer_attached());
    gb.attach_printer(false);
    assert_eq!(exchange(&mut gb, 0x88), 0xFF);
}

#[test]
fn cgb_infrared_port_reads_the_light_and_shows_the_led() {
    let mut gb = GameBoy::new_empty(GbModel::Cgb);
    let mut data = rom(0x00, 0x00, 0x00);
    data[0x143] = 0x80;
    gb.load_rom(&Rom::new(&data)).expect("load");
    gb.cgb.cgb_mode = true;
    gb.cgb.write(0xFF56, 0xC0);
    assert_eq!(gb.cgb.read(0xFF56) & 0x02, 0x02);
    gb.set_ir_light(true);
    assert_eq!(gb.cgb.read(0xFF56) & 0x02, 0x00);
    gb.cgb.write(0xFF56, 0x00);
    assert_eq!(gb.cgb.read(0xFF56) & 0x02, 0x02);
    assert!(!gb.ir_led());
    gb.cgb.write(0xFF56, 0x01);
    assert!(gb.ir_led());
}

#[test]
fn tama5_saves_its_ram_and_clock_after_the_cart_ram() {
    let mut gb = loaded(&rom(0xFD, 0x04, 0x00));
    let file = gb.battery(true, NOW).unwrap().to_file();
    assert_eq!(file.len(), 32 + 32 + 8);
    let mut changed = file.clone();
    changed[0] = 0x42;
    let mut other = loaded(&rom(0xFD, 0x04, 0x00));
    let save = other.battery_from_file(&changed);
    other.put_battery(save, NOW);
    assert_eq!(other.battery(true, NOW).unwrap().to_file()[0], 0x42);
}

#[test]
fn mbc6_saves_ram_then_the_flash() {
    let mut gb = loaded(&rom(0x20, 0x06, 0x03));
    let file = gb.battery(true, NOW).unwrap().to_file();
    assert_eq!(file.len(), 4 * 0x2000 + 0x10_0000);
    let mut changed = file.clone();
    changed[4 * 0x2000 + 5] = 0x17;
    let mut other = loaded(&rom(0x20, 0x06, 0x03));
    let save = other.battery_from_file(&changed);
    other.put_battery(save, NOW);
    other.cartridge.write_naive(0x0C00, 0x01);
    other.cartridge.write_naive(0x2800, 0x08);
    other.cartridge.write_naive(0x2000, 0x00);
    assert_eq!(other.cartridge.read_naive(0x4005), 0x17);
}

#[test]
fn mmm01_boots_the_menu_then_maps_the_game() {
    let mut data = rom(0x01, 0x05, 0x00);
    let menu = data.len() - 0x8000;
    data[menu + 0x104..menu + 0x134].copy_from_slice(&crate::rom::header::NINTENDO_LOGO);
    data[menu + 0x147] = 0x0B;
    let mut gb = loaded(&data);
    assert_eq!(gb.cartridge.read_naive(0x2000), 62);
    assert_eq!(gb.cartridge.read_naive(0x6000), 63);
    gb.cartridge.write_naive(0x2000, 0x20);
    gb.cartridge.write_naive(0x6000, 0x1C << 1);
    gb.cartridge.write_naive(0x0000, 0x40);
    assert_eq!(gb.cartridge.read_naive(0x2000), 0x20);
    assert_eq!(gb.cartridge.read_naive(0x6000), 0x21);
}

#[test]
fn wisdom_tree_switches_32k_by_address() {
    let mut data = rom(0x00, 0x00, 0x00);
    data.resize(0x20000, 0);
    for (bank, chunk) in data.chunks_mut(0x4000).enumerate() {
        chunk[0x2000] = bank as u8;
    }
    data[0x134..0x13F].copy_from_slice(b"WISDOM TREE");
    let mut gb = loaded(&data);
    gb.cartridge.write_naive(0x0002, 0x00);
    assert_eq!(gb.cartridge.read_naive(0x2000), 4);
    assert_eq!(gb.cartridge.read_naive(0x6000), 5);
}

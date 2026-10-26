use crate::ReadMemory;
use crate::gb::{GameBoy, GbModel};

fn spin_boot_rom(model: GbModel) -> Vec<u8> {
    let mut rom = vec![0u8; if model.is_cgb() { 0x900 } else { 0x100 }];
    rom[0..2].copy_from_slice(&[0x18, 0xFE]);
    rom
}

fn read(gb: &mut GameBoy, addr: u16) -> u8 {
    gb.with_bus(|bus| bus.read_naive(addr))
}

fn assert_powered_on_with_lcd_off(gb: &mut GameBoy) {
    assert!(gb.boot_rom.mounted);
    assert_eq!(read(gb, 0xFF40), 0x00);
    for _ in 0..20_000 {
        gb.step();
    }
    assert_eq!(read(gb, 0xFF44), 0);
    assert_eq!(read(gb, 0xFF41) & 0x03, 0);
    assert_eq!(read(gb, 0xFF0F) & 0x01, 0);
}

#[test]
fn boot_rom_starts_with_the_lcd_off() {
    for model in [GbModel::Dmg, GbModel::Cgb] {
        let mut gb = GameBoy::new(model, Some(spin_boot_rom(model)), 0);
        assert_powered_on_with_lcd_off(&mut gb);
    }
}

#[test]
fn soft_reset_with_boot_rom_turns_the_lcd_off() {
    for model in [GbModel::Dmg, GbModel::Cgb] {
        let mut gb = GameBoy::new(model, Some(spin_boot_rom(model)), 0);
        gb.soft_reset();
        assert_powered_on_with_lcd_off(&mut gb);
    }
}

#[test]
fn without_boot_rom_the_lcd_is_on() {
    let mut gb = GameBoy::new_empty(GbModel::Dmg);
    assert_eq!(read(&mut gb, 0xFF40), 0x91);
}

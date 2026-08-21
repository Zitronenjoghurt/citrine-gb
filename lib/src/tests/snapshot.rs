use crate::gb::{GameBoy, GbModel};
use crate::rom::Rom;
use crate::{ReadMemory, WriteMemory};

fn test_rom() -> Vec<u8> {
    let mut data = vec![0u8; 0x8000];
    data[0x0147] = 0x03;
    data[0x0148] = 0x00;
    data[0x0149] = 0x02;
    data[0x0100] = 0xAB;
    data[0x3FFF] = 0xCD;
    data
}

fn loaded() -> GameBoy {
    let data = test_rom();
    let rom = Rom::new(&data);
    let mut gb = GameBoy::new_empty(GbModel::Dmg);
    gb.load_rom(&rom).expect("load");
    gb
}

#[test]
fn restored_snapshot_can_read_rom() {
    let mut gb = loaded();
    for _ in 0..1000 {
        gb.step();
    }

    let dump = gb.dump_full().expect("dump");
    let rom_data = test_rom();
    let restored = GameBoy::from_dump(&dump, &Rom::new(&rom_data)).expect("restore");

    assert_eq!(restored.cartridge.read_naive(0x0100), 0xAB);
    assert_eq!(restored.cartridge.read_naive(0x3FFF), 0xCD);
}

#[test]
fn restored_snapshot_keeps_cpu_state() {
    let mut gb = loaded();
    for _ in 0..1000 {
        gb.step();
    }
    let (pc, sp, a) = (gb.cpu.pc, gb.cpu.sp, gb.cpu.a);

    let dump = gb.dump_full().expect("dump");
    let rom_data = test_rom();
    let restored = GameBoy::from_dump(&dump, &Rom::new(&rom_data)).expect("restore");

    assert_eq!(restored.cpu.pc, pc);
    assert_eq!(restored.cpu.sp, sp);
    assert_eq!(restored.cpu.a, a);
}

#[test]
fn restored_snapshot_keeps_cartridge_ram() {
    let mut gb = loaded();
    gb.cartridge.write_naive(0x0000, 0x0A);
    for (i, addr) in (0xA000..0xA010u16).enumerate() {
        gb.cartridge.write_naive(addr, 0x40 + i as u8);
    }

    let dump = gb.dump_full().expect("dump");
    let rom_data = test_rom();
    let mut restored = GameBoy::from_dump(&dump, &Rom::new(&rom_data)).expect("restore");

    restored.cartridge.write_naive(0x0000, 0x0A);
    for (i, addr) in (0xA000..0xA010u16).enumerate() {
        assert_eq!(
            restored.cartridge.read_naive(addr),
            0x40 + i as u8,
            "SRAM byte {i} did not survive the round trip"
        );
    }
}

#[test]
fn a_restored_machine_keeps_running() {
    let mut gb = loaded();
    for _ in 0..1000 {
        gb.step();
    }
    let dump = gb.dump_full().expect("dump");
    let rom_data = test_rom();
    let mut restored = GameBoy::from_dump(&dump, &Rom::new(&rom_data)).expect("restore");

    for _ in 0..1000 {
        restored.step();
    }
}

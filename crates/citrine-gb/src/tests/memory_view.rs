use crate::ReadMemory;
use crate::WriteMemory;
use crate::debug::Debug;
use crate::debug::breakpoints::{Access, Watch};
use crate::debug::code::{Address, BlockId};
use crate::debug::memory::MemoryBlock;
use crate::debug::step::Stop;
use crate::gb::{GameBoy, GbModel};
use crate::rom::Rom;

const TOBU: &[u8] = include_bytes!("../../../citrine-gb-app/homebrew/tobu.gb");
const UCITY: &[u8] = include_bytes!("../../../citrine-gb-app/homebrew/ucity.gbc");

fn after_frames(data: &[u8], model: GbModel, frames: usize) -> GameBoy {
    let mut gb = GameBoy::new_empty(model);
    gb.load_rom_with_model(&Rom::new(data), model)
        .expect("load");
    for _ in 0..frames {
        gb.run_frame();
    }
    gb
}

fn blocks(gb: &GameBoy) -> Vec<MemoryBlock> {
    let mut out = Vec::new();
    gb.memory_blocks(&mut out);
    out
}

fn block<'a>(blocks: &'a [MemoryBlock], name: &str) -> &'a MemoryBlock {
    blocks
        .iter()
        .find(|block| block.name == name)
        .unwrap_or_else(|| panic!("no block {name}"))
}

fn assert_peek_matches_the_bus(gb: &mut GameBoy) {
    gb.with_bus(|bus| bus.write_naive(0x0000, 0x0A));
    let cart_ram_mapped = gb.address_of(0xA000).is_some();
    for block in blocks(gb) {
        let mut checked = 0;
        for offset in 0..block.size {
            let cpu = (block.start + offset) as u16;
            let Some(at) = gb.address_of(cpu) else {
                continue;
            };
            if at.block != block.id || !block.holds(at.bank) {
                continue;
            }
            let mut peeked = [0];
            gb.peek(at, &mut peeked);
            let read = gb.with_bus(|bus| bus.read_naive(cpu));
            assert_eq!(peeked[0], read, "{} at {at}", block.name);
            checked += 1;
        }
        if block.id != BlockId::CartRam || cart_ram_mapped {
            assert!(checked > 0, "{} was never mapped", block.name);
        }
    }
}

#[test]
fn peek_equals_bus_reads_in_every_block_of_a_game_boy() {
    let mut gb = after_frames(TOBU, GbModel::Dmg, 120);
    let names: Vec<_> = blocks(&gb).iter().map(|block| block.name).collect();
    assert_eq!(
        names,
        [
            "ROM bank 0",
            "Switchable ROM",
            "VRAM",
            "Cart RAM",
            "WRAM bank 0",
            "WRAM bank 1",
            "OAM",
            "I/O",
            "HRAM"
        ]
    );
    assert_peek_matches_the_bus(&mut gb);
}

#[test]
fn peek_equals_bus_reads_in_every_bank_of_a_game_boy_color() {
    let mut gb = after_frames(UCITY, GbModel::Cgb, 120);
    let all = blocks(&gb);
    assert_eq!(block(&all, "VRAM").banks, 2);
    let wram = block(&all, "Switchable WRAM");
    assert_eq!((wram.first_bank, wram.banks), (1, 7));
    assert!(block(&all, "Cart RAM").writable);
    assert!(!block(&all, "Switchable ROM").writable);
    for (vbk, svbk) in [(0, 1), (1, 3), (0, 7)] {
        gb.with_bus(|bus| {
            bus.write_naive(0xFF4F, vbk);
            bus.write_naive(0xFF70, svbk);
        });
        assert_peek_matches_the_bus(&mut gb);
    }
}

#[test]
fn the_boot_rom_is_a_block_only_while_it_is_mounted() {
    let mut boot = vec![0u8; 0x100];
    boot[0..2].copy_from_slice(&[0x18, 0xFE]);
    let mut gb = GameBoy::new(GbModel::Dmg, Some(boot), 0);
    let all = blocks(&gb);
    assert_eq!(block(&all, "Boot ROM").size, 0x100);
    assert_peek_matches_the_bus(&mut gb);

    gb.boot_rom.mounted = false;
    assert!(blocks(&gb).iter().all(|block| block.id != BlockId::BootRom));
}

#[test]
fn peek_reads_any_bank_and_poke_writes_it() {
    let mut gb = after_frames(UCITY, GbModel::Cgb, 30);
    let at = Address::wram(3, 0x10);
    gb.poke(at, &[0xAB, 0xCD]);

    let mut peeked = [0; 2];
    gb.peek(at, &mut peeked);
    assert_eq!(peeked, [0xAB, 0xCD]);

    gb.with_bus(|bus| bus.write_naive(0xFF70, 3));
    assert_eq!(gb.with_bus(|bus| bus.read_naive(0xD011)), 0xCD);
}

#[test]
fn peek_past_the_end_of_a_block_reads_open_bus() {
    let gb = after_frames(TOBU, GbModel::Dmg, 1);
    let mut peeked = [0; 4];
    gb.peek(Address::hram(0x7E), &mut peeked);
    assert_eq!(&peeked[2..], [0xFF, 0xFF]);
}

fn rom_with(code: &[u8]) -> Rom {
    let mut data = vec![0u8; 0x8000];
    data[0x0100..0x0104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]);
    data[0x0150..0x0150 + code.len()].copy_from_slice(code);
    Rom::new(&data)
}

fn console(code: &[u8]) -> GameBoy {
    let mut gb = GameBoy::new_empty(GbModel::Dmg);
    gb.load_rom(&rom_with(code)).expect("load");
    gb.poke(Address::wram(0, 0x123), &[0]);
    gb
}

const WRITES_TWICE: [u8; 10] = [0x3E, 0x42, 0xEA, 0x23, 0xC1, 0xEA, 0x23, 0xC1, 0x18, 0xFE];

fn watch(gb: &mut GameBoy, access: Access) {
    gb.breakpoints().watch(Watch {
        from: Address::wram(0, 0x120),
        len: 8,
        access,
    });
}

#[test]
fn a_write_watchpoint_on_a_wram_byte_stops_at_the_writing_instruction() {
    let mut gb = console(&WRITES_TWICE);
    watch(&mut gb, Access::Write);

    let stop = gb.run_for(100_000);

    assert_eq!(
        stop,
        Stop::Watchpoint {
            at: Address::wram(0, 0x123),
            access: Access::Write,
            by: Some(Address::rom(0, 0x152)),
        }
    );
    assert_eq!(gb.cpu.pc.wrapping_sub(1), 0x155);
    assert_eq!(gb.with_bus(|bus| bus.read_naive(0xC123)), 0x42);
}

#[test]
fn a_watchpoint_stops_a_running_frame() {
    let mut gb = console(&WRITES_TWICE);
    watch(&mut gb, Access::Write);

    gb.run_frame();

    assert!(gb.debugger.hit_breakpoint);
    assert_eq!(gb.cpu.pc.wrapping_sub(1), 0x155);
    assert!(matches!(
        gb.debugger.breakpoints.last_stop(),
        Some(Stop::Watchpoint { .. })
    ));
}

#[test]
fn a_change_watchpoint_ignores_a_write_of_the_same_value() {
    let mut gb = console(&WRITES_TWICE);
    watch(&mut gb, Access::Change);

    assert!(matches!(gb.run_for(100_000), Stop::Watchpoint { .. }));
    assert_eq!(gb.run_for(100_000), Stop::Finished);
}

#[test]
fn a_read_watchpoint_stops_on_a_load_and_not_on_a_store() {
    let mut gb = console(&[0xEA, 0x23, 0xC1, 0x00, 0xFA, 0x23, 0xC1, 0x18, 0xFE]);
    watch(&mut gb, Access::Read);

    let stop = gb.run_for(100_000);

    assert_eq!(
        stop,
        Stop::Watchpoint {
            at: Address::wram(0, 0x123),
            access: Access::Read,
            by: Some(Address::rom(0, 0x154)),
        }
    );
}

#[test]
fn a_removed_watchpoint_no_longer_stops() {
    let mut gb = console(&WRITES_TWICE);
    watch(&mut gb, Access::Write);
    let set = gb.breakpoints().watches()[0];
    gb.breakpoints().unwatch(set);

    assert_eq!(gb.run_for(100_000), Stop::Finished);
    assert!(gb.breakpoints().is_empty());
}

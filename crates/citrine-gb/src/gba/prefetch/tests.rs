use super::*;
use crate::gba::bus::Bus;
use crate::gba::cpu::{Cpu, CpuBus, CpuState};
use Access::{NonSequential, Sequential};
use Width::{Half, Word};

const ROM: u32 = 0x0800_0100;
const WAITCNT: u32 = 0x0400_0204;

fn wait(bits: u16) -> WaitControl {
    let mut wait = WaitControl::default();
    wait.write(bits, 0xFFFF);
    wait
}

fn primed(wait: WaitControl) -> Prefetch {
    let mut prefetch = Prefetch::default();
    prefetch.fetch(wait, ROM, Half, NonSequential);
    prefetch
}

fn halves(prefetch: &mut Prefetch, wait: WaitControl, from: u32, count: u32) -> Vec<u32> {
    (0..count)
        .map(|i| prefetch.fetch(wait, from + 2 * i, Half, Sequential))
        .collect()
}

#[test]
fn a_miss_costs_a_normal_access() {
    let mut prefetch = Prefetch::default();
    assert_eq!(prefetch.fetch(wait(0x4014), ROM, Half, NonSequential), 4);
    assert_eq!(
        prefetch.fetch(wait(0x4014), ROM + 0x40, Word, NonSequential),
        6
    );
}

#[test]
fn straight_line_code_without_free_cycles_costs_sequential_reads() {
    let wait = wait(0x4014);
    let mut prefetch = primed(wait);
    assert_eq!(halves(&mut prefetch, wait, ROM + 2, 3), [2, 2, 2]);
}

#[test]
fn internal_cycles_fill_the_buffer() {
    let wait = wait(0x4014);
    let mut prefetch = primed(wait);
    prefetch.run(wait, 6);
    assert_eq!(halves(&mut prefetch, wait, ROM + 2, 4), [1, 1, 1, 1]);
}

#[test]
fn a_halfword_in_flight_costs_only_its_remaining_cycles() {
    let wait = wait(0x4000);
    let mut prefetch = primed(wait);
    prefetch.run(wait, 1);
    assert_eq!(prefetch.fetch(wait, ROM + 2, Half, Sequential), 2);
}

#[test]
fn an_arm_word_takes_two_halfwords_in_one_cycle() {
    let wait = wait(0x4014);
    let mut prefetch = Prefetch::default();
    prefetch.fetch(wait, ROM, Word, NonSequential);
    prefetch.run(wait, 4);
    assert_eq!(prefetch.fetch(wait, ROM + 4, Word, Sequential), 1);
}

#[test]
fn an_arm_word_with_nothing_buffered_costs_a_sequential_word() {
    let wait = wait(0x4014);
    let mut prefetch = Prefetch::default();
    prefetch.fetch(wait, ROM, Word, NonSequential);
    assert_eq!(prefetch.fetch(wait, ROM + 4, Word, Sequential), 4);
}

#[test]
fn a_branch_to_the_next_address_with_nothing_buffered_is_non_sequential() {
    let wait = wait(0x4014);
    let mut prefetch = primed(wait);
    prefetch.run(wait, 1);
    prefetch.branch();
    assert_eq!(prefetch.fetch(wait, ROM + 2, Half, NonSequential), 4);
}

#[test]
fn a_branch_to_a_buffered_address_reads_from_the_buffer() {
    let wait = wait(0x4014);
    let mut prefetch = primed(wait);
    prefetch.run(wait, 6);
    prefetch.branch();
    assert_eq!(prefetch.fetch(wait, ROM + 2, Half, NonSequential), 1);
}

#[test]
fn a_branch_elsewhere_empties_the_buffer() {
    let wait = wait(0x4014);
    let mut prefetch = primed(wait);
    prefetch.run(wait, 6);
    prefetch.branch();
    assert_eq!(prefetch.fetch(wait, ROM + 6, Half, NonSequential), 4);
    assert_eq!(prefetch.fetch(wait, ROM + 8, Half, Sequential), 2);
}

#[test]
fn a_hit_takes_its_halfword_before_the_cycle_that_would_fill_the_buffer() {
    let wait = wait(0x4014);
    let mut prefetch = primed(wait);
    prefetch.run(wait, 15);
    assert_eq!(halves(&mut prefetch, wait, ROM + 2, 8), [1; 8]);
    assert_eq!(prefetch.fetch(wait, ROM + 18, Half, Sequential), 1);
}

#[test]
fn a_full_buffer_pauses_until_drained_then_restarts_non_sequential() {
    let wait = wait(0x4014);
    let mut prefetch = primed(wait);
    prefetch.run(wait, 100);
    assert_eq!(halves(&mut prefetch, wait, ROM + 2, 8), [1; 8]);
    assert_eq!(prefetch.fetch(wait, ROM + 18, Half, Sequential), 4);
}

#[test]
fn the_buffer_stops_at_a_128k_page() {
    let wait = wait(0x4014);
    let mut prefetch = Prefetch::default();
    prefetch.fetch(wait, 0x0801_FFF8, Half, NonSequential);
    prefetch.run(wait, 99);
    assert_eq!(halves(&mut prefetch, wait, 0x0801_FFFA, 3), [1, 1, 1]);
    assert_eq!(prefetch.fetch(wait, 0x0802_0000, Half, Sequential), 4);
}

#[test]
fn at_a_128k_page_the_buffer_keeps_fetching_without_filling() {
    let wait = wait(0x4014);
    let mut prefetch = Prefetch::default();
    prefetch.fetch(wait, 0x0801_FFF8, Half, NonSequential);
    prefetch.run(wait, 100);
    assert_eq!(halves(&mut prefetch, wait, 0x0801_FFFA, 3), [1, 1, 1]);
    assert_eq!(prefetch.fetch(wait, 0x0802_0000, Half, Sequential), 1 + 4);
}

#[test]
fn a_branch_to_a_128k_page_on_the_last_cycle_of_a_stuck_fetch_costs_one_more() {
    let wait = wait(0x4014);
    let mut prefetch = Prefetch::default();
    prefetch.fetch(wait, 0x0801_FFFC, Half, NonSequential);
    prefetch.run(wait, 5);
    assert_eq!(
        prefetch.fetch(wait, 0x0802_0000, Half, NonSequential),
        1 + 4
    );
}

#[test]
fn code_outside_the_game_pak_leaves_the_buffer_filling() {
    let wait = wait(0x4014);
    let mut prefetch = primed(wait);
    prefetch.run(wait, 6);
    assert_eq!(prefetch.fetch(wait, 0x0300_0000, Half, NonSequential), 1);
    assert_eq!(prefetch.fetch(wait, ROM + 2, Half, NonSequential), 1);
}

#[test]
fn with_bit_14_off_every_fetch_costs_the_wait_table() {
    let wait = wait(0x0014);
    let mut prefetch = primed(wait);
    prefetch.run(wait, 100);
    assert_eq!(halves(&mut prefetch, wait, ROM + 2, 3), [2, 2, 2]);
    assert_eq!(prefetch.fetch(wait, ROM + 8, Half, NonSequential), 4);
}

fn bus_with(waitcnt: u16) -> Bus {
    let mut bus = Bus::new(&[0; 0x200]);
    bus.write16(WAITCNT, waitcnt);
    bus
}

#[test]
fn work_ram_reads_fill_the_buffer() {
    let mut bus = bus_with(0x4014);
    bus.code_cycles(ROM, Half, NonSequential);
    bus.data_cycles(0x0200_0000, Word, NonSequential);
    assert_eq!(bus.code_cycles(ROM + 2, Half, NonSequential), 1);
}

#[test]
fn game_pak_data_reads_empty_the_buffer() {
    let mut bus = bus_with(0x4014);
    bus.code_cycles(ROM, Half, NonSequential);
    for _ in 0..6 {
        CpuBus::idle(&mut bus);
    }
    bus.data_cycles(ROM + 0x80, Word, NonSequential);
    assert_eq!(bus.code_cycles(ROM + 2, Half, NonSequential), 4);
}

#[test]
fn game_pak_writes_empty_the_buffer() {
    let mut bus = bus_with(0x4014);
    bus.code_cycles(ROM, Half, NonSequential);
    bus.data_cycles(0x0200_0000, Word, NonSequential);
    bus.data_cycles(0x0E00_0000, Width::Byte, NonSequential);
    assert_eq!(bus.code_cycles(ROM + 2, Half, NonSequential), 4);
}

const THUMB_PROGRAM: [u16; 6] = [0x2102, 0x0609, 0x6808, 0xE7FF, 0x46C0, 0xE7FE];

fn thumb_steps(waitcnt: u16) -> Vec<u32> {
    let rom: Vec<u8> = THUMB_PROGRAM.iter().flat_map(|h| h.to_le_bytes()).collect();
    let mut bus = Bus::new(&rom);
    bus.write16(WAITCNT, waitcnt);
    let state = CpuState {
        cpsr: 0x3F,
        pipeline: [0x2102, 0x0609],
        next_fetch: Sequential,
        r: {
            let mut r = [0; 16];
            r[15] = 0x0800_0004;
            r
        },
        ..CpuState::default()
    };
    let mut cpu = Cpu::from_state(&state);
    (0..4).map(|_| cpu.step(&mut bus)).collect()
}

#[test]
fn a_thumb_program_with_bit_14_off_keeps_todays_counts() {
    assert_eq!(thumb_steps(0x0000), [3, 3, 10, 13]);
}

#[test]
fn a_thumb_program_with_bit_14_on_reads_from_the_buffer() {
    assert_eq!(thumb_steps(0x4000), [3, 3, 10, 10]);
}

#[test]
fn a_branch_away_abandons_the_halfword_in_flight() {
    let wait = wait(0x4000);
    let mut prefetch = primed(wait);
    prefetch.run(wait, 1);
    assert_eq!(prefetch.fetch(wait, ROM + 0x40, Half, NonSequential), 5);
}

#[test]
fn a_branch_away_on_the_last_cycle_of_a_fetch_costs_one_more() {
    let wait = wait(0x4000);
    let mut prefetch = primed(wait);
    prefetch.run(wait, 2);
    assert_eq!(prefetch.fetch(wait, ROM + 0x40, Half, NonSequential), 1 + 5);
}

#[test]
fn a_branch_away_between_halfwords_costs_nothing_extra() {
    let wait = wait(0x4000);
    let mut prefetch = primed(wait);
    prefetch.run(wait, 3);
    assert_eq!(prefetch.fetch(wait, ROM + 0x40, Half, NonSequential), 5);
}

#[test]
fn a_game_pak_data_read_on_the_last_cycle_of_a_fetch_costs_one_more() {
    let mut bus = bus_with(0x4014);
    bus.code_cycles(ROM, Half, NonSequential);
    CpuBus::idle(&mut bus);
    assert_eq!(bus.data_cycles(ROM + 0x80, Half, NonSequential), 1 + 4);
}

#[test]
fn a_game_pak_data_read_abandons_the_halfword_in_flight() {
    let mut bus = bus_with(0x4000);
    bus.code_cycles(ROM, Half, NonSequential);
    CpuBus::idle(&mut bus);
    assert_eq!(bus.data_cycles(ROM + 0x80, Half, NonSequential), 5);
}

#[test]
fn a_branch_to_the_address_being_fetched_pays_only_the_rest_of_the_fetch() {
    let wait = wait(0x4014);
    let mut prefetch = primed(wait);
    prefetch.run(wait, 1);
    assert_eq!(prefetch.fetch(wait, ROM + 2, Half, Sequential), 1);
    prefetch.run(wait, 1);
    prefetch.branch();
    assert_eq!(prefetch.fetch(wait, ROM + 4, Half, NonSequential), 1);
}

#[test]
fn a_branch_in_the_first_fetch_after_a_miss_pays_a_full_access() {
    let wait = wait(0x4014);
    let mut prefetch = primed(wait);
    prefetch.run(wait, 1);
    prefetch.branch();
    assert_eq!(prefetch.fetch(wait, ROM + 2, Half, NonSequential), 4);
}

#[test]
fn a_misaligned_arm_fetch_never_hits_the_buffer() {
    let wait = wait(0x4014);
    let mut prefetch = Prefetch::default();
    prefetch.fetch(wait, ROM + 2, Word, NonSequential);
    prefetch.run(wait, 100);
    assert_eq!(
        prefetch.fetch(wait, ROM + 6, Word, Sequential),
        wait.cycles(ROM + 6, Word, Sequential)
    );
}

#[test]
fn a_return_from_the_bios_to_other_rom_code_pays_the_cycle_the_buffer_holds_the_bus() {
    let wait = wait(0x4014);
    let mut prefetch = primed(wait);
    prefetch.run(wait, 2);
    assert_eq!(prefetch.fetch(wait, 0x0000_0128, Word, Sequential), 1);
    prefetch.branch();
    assert_eq!(
        prefetch.fetch(wait, ROM + 0x40, Word, NonSequential),
        1 + wait.cycles(ROM + 0x40, Word, NonSequential)
    );
}

#[test]
fn code_from_sram_drops_the_buffer() {
    let wait = wait(0x4014);
    let mut prefetch = primed(wait);
    prefetch.run(wait, 6);
    prefetch.fetch(wait, 0x0E00_0000, Half, NonSequential);
    assert_eq!(prefetch.fetch(wait, ROM + 2, Half, NonSequential), 4);
}

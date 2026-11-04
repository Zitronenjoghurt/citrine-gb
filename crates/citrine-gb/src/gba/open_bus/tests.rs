use crate::gba::bus::Bus;
use crate::gba::cpu::CpuBus;
use crate::gba::wait::{Access, Width};

const UNMAPPED: [u32; 3] = [0x0000_4000, 0x0100_0000, 0x1000_0000];

fn loaded_bus() -> Bus {
    let rom: Vec<u8> = (0..0x100).map(|i| i as u8).collect();
    let mut bus = Bus::new(&rom);
    let bios: Vec<u8> = (0..0x200).map(|i| (i as u8) ^ 0xB0).collect();
    bus.load_bios(&bios);
    for base in [
        0x0200_0000,
        0x0300_0000,
        0x0500_0000,
        0x0600_0000,
        0x0700_0000,
    ] {
        for i in 0..0x10 {
            bus.write16(base + 2 * i, (base >> 16) as u16 | i as u16);
        }
    }
    bus
}

fn fetch(bus: &mut Bus, addr: u32, width: Width) {
    CpuBus::fetch(bus, addr, width, Access::Sequential);
}

fn read(bus: &mut Bus, addr: u32, width: Width) -> u32 {
    CpuBus::read(bus, addr, width, Access::NonSequential)
}

fn thumb_run(bus: &mut Bus, addr: u32) {
    fetch(bus, addr - 2, Width::Half);
    fetch(bus, addr, Width::Half);
}

fn half(bus: &Bus, addr: u32) -> u32 {
    u32::from(bus.read16(addr))
}

#[test]
fn arm_reads_the_last_fetched_opcode_from_every_region() {
    for code in [
        0x0000_0010,
        0x0200_0008,
        0x0300_0008,
        0x0600_0008,
        0x0800_0008,
    ] {
        let mut bus = loaded_bus();
        fetch(&mut bus, code, Width::Word);
        let opcode = bus.read32(code);
        for addr in UNMAPPED {
            assert_eq!(
                read(&mut bus, addr, Width::Word),
                opcode,
                "{code:08X} {addr:08X}"
            );
        }
    }
}

#[test]
fn narrow_reads_take_the_addressed_lane() {
    let mut bus = loaded_bus();
    fetch(&mut bus, 0x0800_0008, Width::Word);
    assert_eq!(read(&mut bus, 0x0100_0002, Width::Half), 0x0B0A);
    assert_eq!(read(&mut bus, 0x0100_0000, Width::Half), 0x0908);
    assert_eq!(read(&mut bus, 0x0100_0003, Width::Byte), 0x0B);
    assert_eq!(read(&mut bus, 0x0100_0001, Width::Byte), 0x09);
}

#[test]
fn thumb_repeats_the_halfword_from_rom_ewram_palette_and_vram() {
    for code in [
        0x0800_0006,
        0x0800_0008,
        0x0200_0006,
        0x0500_0008,
        0x0600_0006,
    ] {
        let mut bus = loaded_bus();
        thumb_run(&mut bus, code);
        let opcode = half(&bus, code);
        assert_eq!(
            read(&mut bus, 0x0100_0000, Width::Word),
            opcode << 16 | opcode,
            "{code:08X}"
        );
    }
}

#[test]
fn thumb_from_bios_and_oam_pairs_with_the_neighbour_in_the_word() {
    for code in [0x0000_0108, 0x0700_0008] {
        let mut bus = loaded_bus();
        thumb_run(&mut bus, code);
        let expected = half(&bus, code + 2) << 16 | half(&bus, code);
        assert_eq!(
            read(&mut bus, 0x0100_0000, Width::Word),
            expected,
            "{code:08X}"
        );

        let mut bus = loaded_bus();
        thumb_run(&mut bus, code + 2);
        let expected = half(&bus, code + 2) << 16 | half(&bus, code);
        assert_eq!(
            read(&mut bus, 0x0100_0000, Width::Word),
            expected,
            "{code:08X}"
        );
    }
}

#[test]
fn thumb_from_iwram_puts_the_older_halfword_on_top_when_aligned() {
    let mut bus = loaded_bus();
    thumb_run(&mut bus, 0x0300_0008);
    let expected = half(&bus, 0x0300_0006) << 16 | half(&bus, 0x0300_0008);
    assert_eq!(read(&mut bus, 0x0100_0000, Width::Word), expected);

    let mut bus = loaded_bus();
    thumb_run(&mut bus, 0x0300_000A);
    let expected = half(&bus, 0x0300_000A) << 16 | half(&bus, 0x0300_0008);
    assert_eq!(read(&mut bus, 0x0100_0000, Width::Word), expected);
}

#[test]
fn thumb_from_iwram_after_an_iwram_load_keeps_the_loaded_word_in_the_old_lane() {
    let mut bus = loaded_bus();
    fetch(&mut bus, 0x0300_0006, Width::Half);
    read(&mut bus, 0x0300_0010, Width::Word);
    fetch(&mut bus, 0x0300_0008, Width::Half);
    let expected = half(&bus, 0x0300_0012) << 16 | half(&bus, 0x0300_0008);
    assert_eq!(read(&mut bus, 0x0100_0000, Width::Word), expected);

    let mut bus = loaded_bus();
    fetch(&mut bus, 0x0300_0008, Width::Half);
    read(&mut bus, 0x0300_0010, Width::Word);
    fetch(&mut bus, 0x0300_000A, Width::Half);
    let expected = half(&bus, 0x0300_000A) << 16 | half(&bus, 0x0300_0010);
    assert_eq!(read(&mut bus, 0x0100_0000, Width::Word), expected);
}

#[test]
fn thumb_from_iwram_after_a_halfword_load_replaces_only_its_lane() {
    let mut bus = loaded_bus();
    fetch(&mut bus, 0x0300_0006, Width::Half);
    read(&mut bus, 0x0300_0010, Width::Half);
    fetch(&mut bus, 0x0300_0008, Width::Half);
    let expected = half(&bus, 0x0300_0006) << 16 | half(&bus, 0x0300_0008);
    assert_eq!(read(&mut bus, 0x0100_0000, Width::Word), expected);
}

#[test]
fn bios_reads_from_outside_return_the_last_bios_fetch() {
    let mut bus = loaded_bus();
    fetch(&mut bus, 0x0000_0190, Width::Word);
    fetch(&mut bus, 0x0800_0000, Width::Word);
    let latched = bus.read32(0x0000_0190);
    assert_eq!(read(&mut bus, 0x0000_0000, Width::Word), latched);
    assert_eq!(read(&mut bus, 0x0000_0042, Width::Half), latched >> 16);
    assert_eq!(
        read(&mut bus, 0x0000_0101, Width::Byte),
        latched >> 8 & 0xFF
    );
}

#[test]
fn bios_reads_from_inside_see_the_image() {
    let mut bus = loaded_bus();
    fetch(&mut bus, 0x0000_0190, Width::Word);
    assert_eq!(
        read(&mut bus, 0x0000_0040, Width::Word),
        bus.read32(0x0000_0040)
    );
}

#[test]
fn bios_latch_starts_at_the_opcode_the_boot_ends_on() {
    let mut bus = loaded_bus();
    fetch(&mut bus, 0x0800_0000, Width::Word);
    assert_eq!(
        read(&mut bus, 0x0000_0000, Width::Word),
        bus.read32(0x0000_00E4)
    );
}

#[test]
fn thumb_bios_fetch_latches_the_whole_word() {
    let mut bus = loaded_bus();
    thumb_run(&mut bus, 0x0000_0102);
    fetch(&mut bus, 0x0800_0000, Width::Half);
    assert_eq!(
        read(&mut bus, 0x0000_0000, Width::Word),
        bus.read32(0x0000_0100)
    );
}

#[test]
fn debug_reads_still_see_the_bios_and_zero_when_unmapped() {
    let mut bus = loaded_bus();
    fetch(&mut bus, 0x0800_0008, Width::Word);
    assert_eq!(bus.read32(0x0000_0000), 0xB3B2_B1B0);
    assert_eq!(bus.read32(0x0100_0000), 0);
}

fn write(bus: &mut Bus, addr: u32, width: Width, value: u32) {
    CpuBus::write(bus, addr, width, value, Access::NonSequential);
}

fn dma(bus: &mut Bus, source: u32, dest: u32, control: u16) {
    let base = 0x0400_00BC;
    bus.write32(base, source);
    bus.write32(base + 4, dest);
    bus.write16(base + 8, 1);
    bus.write16(base + 10, control | 0x8000);
    bus.run_dma();
}

#[test]
fn a_data_load_leaves_its_value_on_the_bus() {
    let mut bus = loaded_bus();
    fetch(&mut bus, 0x0800_0008, Width::Word);
    let loaded = read(&mut bus, 0x0200_0004, Width::Half);
    assert_eq!(
        read(&mut bus, 0x0100_0000, Width::Word),
        loaded << 16 | loaded
    );
    read(&mut bus, 0x0800_0006, Width::Byte);
    assert_eq!(read(&mut bus, 0x0100_0000, Width::Word), 0x0606_0606);
}

#[test]
fn a_misaligned_halfword_load_puts_the_whole_aligned_halfword_on_the_bus() {
    let mut bus = loaded_bus();
    read(&mut bus, 0x0800_0005, Width::Half);
    assert_eq!(read(&mut bus, 0x0100_0000, Width::Word), 0x0504_0504);
}

#[test]
fn unmapped_reads_leave_the_bus_alone() {
    let mut bus = loaded_bus();
    fetch(&mut bus, 0x0800_0008, Width::Word);
    read(&mut bus, 0x0100_0002, Width::Half);
    read(&mut bus, 0x1000_0000, Width::Word);
    assert_eq!(read(&mut bus, 0x0100_0000, Width::Word), 0x0B0A_0908);
}

#[test]
fn writes_replace_their_lane_of_the_bus() {
    let mut bus = loaded_bus();
    fetch(&mut bus, 0x0800_0008, Width::Word);
    write(&mut bus, 0x0200_0001, Width::Byte, 0xAB);
    assert_eq!(read(&mut bus, 0x0100_0000, Width::Word), 0x0B0A_AB08);
    write(&mut bus, 0x0200_0002, Width::Half, 0xCDEF);
    assert_eq!(read(&mut bus, 0x0100_0000, Width::Word), 0xCDEF_AB08);
    write(&mut bus, 0x0000_0000, Width::Word, 0x1234_5678);
    assert_eq!(read(&mut bus, 0x0100_0000, Width::Word), 0x1234_5678);
}

#[test]
fn an_iwram_write_also_lands_in_the_iwram_lanes() {
    let mut bus = loaded_bus();
    fetch(&mut bus, 0x0300_0006, Width::Half);
    write(&mut bus, 0x0300_0000, Width::Half, 0x1234);
    fetch(&mut bus, 0x0300_000A, Width::Half);
    let expected = half(&bus, 0x0300_000A) << 16 | 0x1234;
    assert_eq!(read(&mut bus, 0x0100_0000, Width::Word), expected);
}

#[test]
fn a_dma_from_unused_io_reads_the_cpu_bus() {
    let mut bus = loaded_bus();
    fetch(&mut bus, 0x0800_0008, Width::Word);
    dma(&mut bus, 0x0400_1000, 0x0200_0020, 0x0400);
    assert_eq!(bus.read32(0x0200_0020), 0x0B0A_0908);
}

#[test]
fn a_halfword_dma_from_unused_io_puts_its_half_on_both_lanes_of_the_cpu_bus() {
    let mut bus = loaded_bus();
    fetch(&mut bus, 0x0800_0008, Width::Word);
    dma(&mut bus, 0x0401_0000, 0x0200_0020, 0);
    assert_eq!(bus.read16(0x0200_0020), 0x0908);
    assert_eq!(read(&mut bus, 0x0100_0000, Width::Word), 0x0908_0908);
}

#[test]
fn a_dma_from_the_bios_copies_its_own_latch_to_the_cpu_bus() {
    let mut bus = loaded_bus();
    dma(&mut bus, 0x0300_0004, 0x0200_0020, 0x0400);
    let moved = bus.read32(0x0300_0004);
    fetch(&mut bus, 0x0800_0008, Width::Word);
    dma(&mut bus, 0x0000_0000, 0x0200_0024, 0x0400);
    assert_eq!(bus.read32(0x0200_0024), moved);
    assert_eq!(read(&mut bus, 0x0100_0000, Width::Word), moved);
}

#[test]
fn a_halfword_dma_with_nothing_to_read_leaves_the_source_half_on_the_bus() {
    let mut bus = loaded_bus();
    bus.write32(0x0300_0010, 0xAAAA_BBBB);
    dma(&mut bus, 0x0300_0010, 0x0200_0020, 0x0400);
    fetch(&mut bus, 0x0800_0008, Width::Word);
    dma(&mut bus, 0x0100_0002, 0x0200_0030, 0);
    assert_eq!(bus.read16(0x0200_0030), 0xBBBB);
    assert_eq!(read(&mut bus, 0x0100_0000, Width::Word), 0xAAAA_BBBB);
}

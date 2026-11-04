use super::*;

fn rom_counting(len: usize) -> Vec<u8> {
    (0..len).map(|i| i as u8).collect()
}

fn bus() -> Bus {
    Bus::new(&rom_counting(0x100))
}

#[test]
fn bios_reads_the_loaded_image_and_ignores_writes() {
    let mut bus = bus();
    bus.load_bios(&[0x11, 0x22, 0x33, 0x44]);
    assert_eq!(bus.read32(0x0000_0000), 0x4433_2211);
    assert_eq!(bus.read16(0x0000_0002), 0x4433);
    assert_eq!(bus.read8(0x0000_0001), 0x22);
    bus.write32(0x0000_0000, 0xDEAD_BEEF);
    assert_eq!(bus.read32(0x0000_0000), 0x4433_2211);
    assert_eq!(bus.read32(0x0000_4000), 0);
}

#[test]
fn ewram_is_256k_and_mirrors_through_the_region() {
    let mut bus = bus();
    bus.write32(0x0200_0000, 0x1122_3344);
    bus.write8(0x0203_FFFF, 0xAB);
    assert_eq!(bus.read32(0x0204_0000), 0x1122_3344);
    assert_eq!(bus.read32(0x02FC_0000), 0x1122_3344);
    assert_eq!(bus.read8(0x02FF_FFFF), 0xAB);
    assert_eq!(bus.read8(0x0200_0000), 0x44);
}

#[test]
fn iwram_is_32k_and_mirrors_through_the_region() {
    let mut bus = bus();
    bus.write32(0x0300_7FFC, 0x0300_1234);
    assert_eq!(bus.read32(0x03FF_FFFC), 0x0300_1234);
    assert_eq!(bus.read32(0x0300_FFFC), 0x0300_1234);
    bus.write16(0x0300_8000, 0xBEEF);
    assert_eq!(bus.read16(0x0300_0000), 0xBEEF);
}

#[test]
fn wide_accesses_align_down() {
    let mut bus = bus();
    bus.write32(0x0300_0002, 0x1122_3344);
    assert_eq!(bus.read32(0x0300_0000), 0x1122_3344);
    assert_eq!(bus.read32(0x0300_0003), 0x1122_3344);
    assert_eq!(bus.read16(0x0300_0001), 0x3344);
    bus.write16(0x0300_0001, 0xAAAA);
    assert_eq!(bus.read32(0x0300_0000), 0x1122_AAAA);
}

#[test]
fn io_keeps_plain_registers_and_ends_at_3ff() {
    let mut bus = bus();
    bus.write16(0x0400_0000, 0x0403);
    assert_eq!(bus.read16(0x0400_0000), 0x0403);
    bus.write8(0x0400_0001, 0x1F);
    assert_eq!(bus.read16(0x0400_0000), 0x1F03);
    bus.write32(0x0400_0400, 0xFFFF_FFFF);
    assert_eq!(bus.read32(0x0400_0400), 0);
}

#[test]
fn waitcnt_reads_and_writes_through_the_bus() {
    let mut bus = bus();
    bus.write16(0x0400_0204, 0x4317);
    assert_eq!(bus.read16(0x0400_0204), 0x4317);
    assert_eq!(bus.wait().read(), 0x4317);
    assert_eq!(
        bus.cycles(0x0800_0000, Width::Half, Access::NonSequential),
        4
    );
    bus.write8(0x0400_0205, 0x00);
    assert_eq!(bus.read16(0x0400_0204), 0x0017);
    bus.write32(0x0400_0204, 0xFFFF_FFFF);
    assert_eq!(bus.read8(0x0400_0205), 0x5F);
}

#[test]
fn palette_is_1k_mirrored_and_byte_writes_fill_the_halfword() {
    let mut bus = bus();
    bus.write16(0x0500_0000, 0x7FFF);
    assert_eq!(bus.read16(0x0500_0400), 0x7FFF);
    assert_eq!(bus.read16(0x05FF_FC00), 0x7FFF);
    bus.write8(0x0500_0003, 0x12);
    assert_eq!(bus.read16(0x0500_0002), 0x1212);
    bus.write32(0x0500_03FC, 0xAABB_CCDD);
    assert_eq!(bus.read32(0x0500_07FC), 0xAABB_CCDD);
}

#[test]
fn vram_is_96k_with_the_upper_32k_mirroring_the_obj_bank() {
    let mut bus = bus();
    bus.write16(0x0601_0000, 0x1234);
    assert_eq!(bus.read16(0x0601_8000), 0x1234);
    bus.write16(0x0601_FFFE, 0x5678);
    assert_eq!(bus.read16(0x0601_7FFE), 0x5678);
    bus.write32(0x0600_0000, 0xCAFE_F00D);
    assert_eq!(bus.read32(0x0602_0000), 0xCAFE_F00D);
    assert_eq!(bus.read32(0x06FE_0000), 0xCAFE_F00D);
}

#[test]
fn vram_byte_writes_fill_bg_and_skip_obj_in_tiled_modes() {
    let mut bus = bus();
    bus.write8(0x0600_FFFF, 0x42);
    assert_eq!(bus.read16(0x0600_FFFE), 0x4242);
    bus.write8(0x0601_0000, 0x42);
    assert_eq!(bus.read16(0x0601_0000), 0);
}

#[test]
fn vram_byte_writes_reach_further_in_bitmap_modes() {
    let mut bus = bus();
    bus.write16(0x0400_0000, 0x0003);
    bus.write8(0x0601_3FFE, 0x42);
    assert_eq!(bus.read16(0x0601_3FFE), 0x4242);
    bus.write8(0x0601_4000, 0x42);
    assert_eq!(bus.read16(0x0601_4000), 0);
}

#[test]
fn oam_is_1k_mirrored_and_ignores_byte_writes() {
    let mut bus = bus();
    bus.write32(0x0700_0000, 0x0102_0304);
    assert_eq!(bus.read32(0x0700_0400), 0x0102_0304);
    assert_eq!(bus.read32(0x07FF_FC00), 0x0102_0304);
    bus.write8(0x0700_0000, 0xFF);
    assert_eq!(bus.read32(0x0700_0000), 0x0102_0304);
    bus.write16(0x0700_0002, 0xFFFF);
    assert_eq!(bus.read32(0x0700_0000), 0xFFFF_0304);
}

#[test]
fn rom_reads_little_endian_in_all_three_windows() {
    let bus = bus();
    for base in [0x0800_0000, 0x0A00_0000, 0x0C00_0000] {
        assert_eq!(bus.read32(base + 4), 0x0706_0504, "{base:08X}");
        assert_eq!(bus.read16(base + 6), 0x0706, "{base:08X}");
        assert_eq!(bus.read8(base + 5), 0x05, "{base:08X}");
    }
    assert_eq!(bus.read8(0x0900_0005), 0);
}

#[test]
fn rom_mirrors_every_32m() {
    let bus = Bus::new(&rom_counting(0x200_0000 + 0x10));
    assert_eq!(bus.read8(0x0900_0001), 0x01);
    assert_eq!(bus.read8(0x0B00_0001), 0x01);
    assert_eq!(bus.read8(0x0D00_00FF), 0xFF);
}

#[test]
fn rom_past_its_end_reads_the_address_halves() {
    let bus = bus();
    assert_eq!(bus.read16(0x0800_0100), 0x0080);
    assert_eq!(bus.read16(0x0800_0102), 0x0081);
    assert_eq!(bus.read32(0x0800_0100), 0x0081_0080);
    assert_eq!(bus.read8(0x0800_0103), 0x00);
    assert_eq!(bus.read16(0x0A01_FFFE), 0xFFFF);
    assert_eq!(bus.read16(0x0802_0000), 0x0000);
}

#[test]
fn rom_ignores_writes() {
    let mut bus = bus();
    bus.write32(0x0800_0000, 0xFFFF_FFFF);
    assert_eq!(bus.read32(0x0800_0000), 0x0302_0100);
}

#[test]
fn sram_is_64k_on_an_8_bit_bus() {
    let mut bus = bus();
    assert_eq!(bus.read8(0x0E00_0000), 0xFF);
    bus.write8(0x0E00_0001, 0x5A);
    assert_eq!(bus.read8(0x0E01_0001), 0x5A);
    assert_eq!(bus.read8(0x0F00_0001), 0x5A);
    assert_eq!(bus.read16(0x0E00_0001), 0x5A5A);
    assert_eq!(bus.read32(0x0E00_0001), 0x5A5A_5A5A);
}

#[test]
fn sram_wide_writes_store_the_addressed_byte_lane() {
    let mut bus = bus();
    bus.write16(0x0E00_0011, 0xAABB);
    assert_eq!(bus.read8(0x0E00_0011), 0xAA);
    assert_eq!(bus.read8(0x0E00_0010), 0xFF);
    bus.write32(0x0E00_0022, 0x1122_3344);
    assert_eq!(bus.read8(0x0E00_0022), 0x22);
    assert_eq!(bus.read8(0x0E00_0020), 0xFF);
}

#[test]
fn unmapped_reads_zero_and_ignores_writes() {
    let mut bus = bus();
    for addr in [0x0000_4000, 0x0100_0000, 0x1000_0000, 0xFFFF_FFFC] {
        bus.write32(addr, 0xFFFF_FFFF);
        assert_eq!(bus.read32(addr), 0, "{addr:08X}");
    }
}

#[test]
fn rom_is_capped_at_32m() {
    let bus = Bus::new(&vec![0xAA; 0x200_0000 + 4]);
    assert_eq!(bus.rom.len(), 0x200_0000);
}

#[test]
fn keyinput_reads_through_the_bus_and_ignores_writes() {
    let mut bus = bus();
    assert_eq!(bus.read16(0x0400_0130), 0x03FF);
    bus.set_keys(Keys::A | Keys::L);
    assert_eq!(bus.read16(0x0400_0130), 0x01FE);
    assert_eq!(bus.read8(0x0400_0131), 0x01);
    bus.write16(0x0400_0130, 0x0000);
    assert_eq!(bus.read16(0x0400_0130), 0x01FE);
}

#[test]
fn keycnt_shares_a_word_with_keyinput() {
    let mut bus = bus();
    bus.write32(0x0400_0130, 0xC00F_0000);
    assert_eq!(bus.read32(0x0400_0130), 0xC00F_03FF);
    assert_eq!(bus.keypad().keycnt(), 0xC00F);
}

#[test]
fn keypad_irq_sets_if_bit_12_in_any_of_mode() {
    let mut bus = bus();
    bus.write16(0x0400_0132, 0x4000 | Keys::START.bits());
    bus.set_keys(Keys::A);
    assert_eq!(bus.read16(0x0400_0202), 0);
    bus.set_keys(Keys::START);
    assert_eq!(bus.read16(0x0400_0202), 0);
    bus.step_video(1);
    assert_eq!(bus.read16(0x0400_0202), Irq::KEYPAD.bits());
}

#[test]
fn keypad_irq_in_all_of_mode_waits_for_every_key() {
    let mut bus = bus();
    let combo = Keys::A | Keys::B | Keys::SELECT | Keys::START;
    bus.write16(0x0400_0132, 0xC000 | combo.bits());
    bus.set_keys(Keys::A | Keys::B | Keys::START);
    assert_eq!(bus.interrupts().if_(), 0);
    bus.set_keys(combo);
    bus.step_video(1);
    assert_eq!(bus.interrupts().if_(), Irq::KEYPAD.bits());
}

#[test]
fn keycnt_write_with_the_keys_held_raises_the_irq() {
    let mut bus = bus();
    bus.set_keys(Keys::R);
    bus.write16(0x0400_0132, Keys::R.bits());
    assert_eq!(bus.interrupts().if_(), 0);
    bus.write8(0x0400_0133, 0x41);
    assert_eq!(bus.keypad().keycnt(), 0x4100);
    assert_eq!(bus.interrupts().if_(), 0);
    bus.step_video(1);
    assert_eq!(bus.interrupts().if_(), Irq::KEYPAD.bits());
}

#[test]
fn keypad_irq_is_acknowledged_by_writing_one_to_if() {
    let mut bus = bus();
    bus.write16(0x0400_0200, Irq::KEYPAD.bits());
    bus.write16(0x0400_0208, 1);
    bus.write16(0x0400_0132, 0x4000 | Keys::A.bits());
    bus.set_keys(Keys::A);
    assert!(bus.interrupts().asserted());
    bus.write16(0x0400_0202, Irq::KEYPAD.bits());
    assert_eq!(bus.read16(0x0400_0202), 0);
    assert!(!bus.interrupts().asserted());
}

#[test]
fn ie_if_and_ime_read_back_through_the_bus() {
    let mut bus = bus();
    bus.write32(0x0400_0200, 0xFFFF_FFFF);
    assert_eq!(bus.read32(0x0400_0200), 0x0000_3FFF);
    bus.write32(0x0400_0208, 0xFFFF_FFFF);
    assert_eq!(bus.read16(0x0400_0208), 1);
}

const TM0CNT: u32 = 0x0400_0100;
const DMA_BASE: u32 = 0x0400_00B0;
const IF_ADDR: u32 = 0x0400_0202;

fn dma_registers(bus: &mut Bus, channel: u32, source: u32, dest: u32, count: u16, control: u16) {
    let base = DMA_BASE + 12 * channel;
    bus.write32(base, source);
    bus.write32(base + 4, dest);
    bus.write16(base + 8, count);
    bus.write16(base + 10, control | 0x8000);
}

#[test]
fn a_32_bit_timer_write_starts_from_the_new_reload() {
    let mut bus = bus();
    bus.write32(TM0CNT, 0x00C0_FFFE);
    bus.step(1);
    assert_eq!(bus.read16(TM0CNT), 0);
    assert_eq!(bus.read16(TM0CNT + 2), 0x00C0);
    bus.step(1);
    assert_eq!(bus.read16(TM0CNT), 0xFFFE);
    bus.step(1);
    assert_eq!(bus.read16(TM0CNT), 0xFFFF);
    assert_eq!(bus.read16(IF_ADDR), 0);
    bus.step(1);
    assert_eq!(bus.read16(TM0CNT), 0xFFFE);
    bus.step_video(1);
    assert_eq!(bus.read16(IF_ADDR), 0);
    bus.step(1);
    bus.step_video(1);
    assert_eq!(bus.read16(IF_ADDR), Irq::TIMER0.bits());
}

#[test]
fn timer_registers_take_byte_writes() {
    let mut bus = bus();
    bus.write8(TM0CNT + 4, 0x34);
    bus.write8(TM0CNT + 5, 0x12);
    bus.write8(TM0CNT + 6, 0x80);
    bus.step(2);
    assert_eq!(bus.read16(TM0CNT + 4), 0x1234);
    assert_eq!(bus.read8(TM0CNT + 6), 0x80);
}

#[test]
fn step_without_timers_requests_nothing() {
    let mut bus = bus();
    bus.write16(TM0CNT, 0xFFFF);
    bus.write16(TM0CNT + 2, 0x0040);
    assert_eq!(bus.timers_step(100_000), Overflows::default());
    assert_eq!(bus.read16(IF_ADDR), 0);
}

#[test]
fn immediate_dma_copies_and_reports_its_cycles() {
    let mut bus = bus();
    for i in 0..4 {
        bus.write32(0x0200_0000 + 4 * i, 0x1111_1111 * (i + 1));
    }
    dma_registers(&mut bus, 3, 0x0200_0000, 0x0300_0000, 4, 0x4400);
    assert!(bus.dma_active());
    assert_eq!(bus.run_dma(), 2 + 2 + 4 * 6 + 4);
    for i in 0..4 {
        assert_eq!(bus.read32(0x0300_0000 + 4 * i), 0x1111_1111 * (i + 1));
    }
    assert!(!bus.dma_active());
    assert_eq!(bus.read16(DMA_BASE + 12 * 3 + 10) & 0x8000, 0);
    assert_eq!(bus.interrupts().raised(), Irq::DMA3.bits());
}

#[test]
fn a_dma_irq_shows_in_if_the_cycle_after_the_dma_gives_the_bus_back() {
    let mut bus = bus();
    dma_registers(&mut bus, 3, 0x0200_0000, 0x0300_0000, 4, 0x4400);
    let cycles = bus.run_dma();
    assert_eq!(bus.interrupts().if_at(cycles), 0);
    assert_eq!(bus.interrupts().if_at(cycles + 1), Irq::DMA3.bits());
}

#[test]
fn vblank_dma_runs_only_after_the_vblank_call() {
    let mut bus = bus();
    bus.write16(0x0200_0000, 0xBEEF);
    dma_registers(&mut bus, 1, 0x0200_0000, 0x0300_0000, 1, 0x1000);
    assert_eq!(bus.run_dma(), 0);
    bus.start_dma(Timing::HBlank);
    assert_eq!(bus.run_dma(), 0);
    bus.start_dma(Timing::VBlank);
    assert!(bus.run_dma() > 0);
    assert_eq!(bus.read16(0x0300_0000), 0xBEEF);
}

#[test]
fn channels_started_together_run_lower_first() {
    let mut bus = bus();
    bus.write16(0x0200_0000, 0xAAAA);
    bus.write16(0x0300_0000, 0x5555);
    dma_registers(&mut bus, 2, 0x0300_0000, 0x0300_0010, 1, 0x2000);
    dma_registers(&mut bus, 1, 0x0200_0000, 0x0300_0000, 1, 0x2000);
    bus.start_dma(Timing::HBlank);
    bus.run_dma();
    assert_eq!(bus.read16(0x0300_0010), 0xAAAA);
}

#[test]
fn dma0_masks_rom_down_to_the_bios_and_gets_its_latch() {
    let mut bus = bus();
    bus.load_bios(&[0x11, 0x22]);
    bus.write16(0x0300_0000, 0xFFFF);
    dma_registers(&mut bus, 0, 0x0800_0000, 0x0300_0000, 1, 0);
    dma_registers(&mut bus, 3, 0x0800_0000, 0x0300_0002, 1, 0);
    bus.run_dma();
    assert_eq!(bus.read16(0x0300_0000), 0);
    assert_eq!(bus.read16(0x0300_0002), 0x0100);
}

#[test]
fn dma_from_the_bios_repeats_the_last_word_it_moved() {
    let mut bus = bus();
    bus.load_bios(&[0x11, 0x22, 0x33, 0x44]);
    bus.write32(0x0300_0100, 0xAABB_CCDD);
    dma_registers(&mut bus, 1, 0x0300_0100, 0x0300_0000, 1, 0x0400);
    bus.run_dma();
    dma_registers(&mut bus, 1, 0x0000_0000, 0x0300_0010, 2, 0x0400);
    bus.run_dma();
    assert_eq!(bus.read32(0x0300_0010), 0xAABB_CCDD);
    assert_eq!(bus.read32(0x0300_0014), 0xAABB_CCDD);
}

#[test]
fn halfword_dma_from_unmapped_memory_repeats_the_last_half_on_both_lanes() {
    let mut bus = bus();
    bus.write16(0x0300_0100, 0xCCDD);
    dma_registers(&mut bus, 3, 0x0300_0100, 0x0300_0000, 1, 0);
    bus.run_dma();
    dma_registers(&mut bus, 3, 0x0100_0000, 0x0300_0020, 2, 0);
    bus.run_dma();
    assert_eq!(bus.read32(0x0300_0020), 0xCCDD_CCDD);
}

#[test]
fn each_dma_channel_keeps_its_own_latch() {
    let mut bus = bus();
    bus.write32(0x0300_0100, 0x1111_1111);
    bus.write32(0x0300_0104, 0x2222_2222);
    dma_registers(&mut bus, 1, 0x0300_0100, 0x0300_0000, 1, 0x0400);
    dma_registers(&mut bus, 2, 0x0300_0104, 0x0300_0000, 1, 0x0400);
    bus.run_dma();
    dma_registers(&mut bus, 1, 0x0000_0000, 0x0300_0010, 1, 0x0400);
    bus.run_dma();
    assert_eq!(bus.read32(0x0300_0010), 0x1111_1111);
}

#[test]
fn dma3_writes_to_rom_addresses_change_nothing() {
    let mut bus = bus();
    bus.write16(0x0300_0000, 0xFFFF);
    dma_registers(&mut bus, 3, 0x0300_0000, 0x0800_0000, 1, 0);
    bus.run_dma();
    assert_eq!(bus.read16(0x0800_0000), 0x0100);
}

#[test]
fn special_dma_starts_from_its_hook() {
    let mut bus = bus();
    bus.write32(0x0200_0000, 0x1234_5678);
    dma_registers(&mut bus, 1, 0x0200_0000, 0x0400_00A0, 0, 0x3200);
    bus.start_special_dma(1);
    assert_eq!(bus.run_dma(), 2 + 6 + 3 * 6 + 4);
    assert_eq!(bus.read32(0x0400_00A0), 0);
    assert_ne!(bus.read16(DMA_BASE + 12 + 10) & 0x8000, 0);
}

#[test]
fn an_hblank_dma_takes_over_a_running_dma_between_units() {
    let mut bus = bus();
    let units = 2000;
    for i in 0..units {
        bus.write16(0x0200_0000 + 2 * i, 0xAAAA);
    }
    let last = 0x0300_0000 + 2 * (units - 1);
    dma_registers(&mut bus, 0, last, 0x0300_4000, 1, 0x2000);
    dma_registers(&mut bus, 3, 0x0200_0000, 0x0300_0000, units as u16, 0);
    bus.run_dma();
    assert_eq!(bus.read16(0x0300_4000), 0);
    assert_eq!(bus.read16(last), 0xAAAA);
    assert_eq!(bus.read16(DMA_BASE + 12 * 3 + 10) & 0x8000, 0);
}

fn bus_with_hblank_marker_dma() -> Bus {
    let mut bus = bus();
    bus.write32(0x0200_F000, 0xBEEF_CAFE);
    dma_registers(&mut bus, 0, 0x0200_F000, 0x0300_7000, 1, 0x2400);
    bus
}

#[test]
fn an_hblank_dma_due_when_a_dma_ends_runs_before_the_next_cpu_access() {
    for lead in 1..=3 {
        let mut bus = bus_with_hblank_marker_dma();
        for _ in 0..lead {
            bus.cpu_idle();
        }
        dma_registers(&mut bus, 3, 0x0800_0000, 0x0900_0000, 166, 0);
        let mut addr = 0x0800_0000;
        let mut access = Access::NonSequential;
        while bus.dma_cycles() == 0 {
            bus.code_cycles(addr, Width::Half, access);
            addr += 2;
            access = Access::Sequential;
        }
        assert_eq!(
            bus.read32(0x0300_7000),
            0xBEEF_CAFE,
            "{lead} idle cycles first"
        );
    }
}

#[test]
fn the_first_cpu_access_after_a_dma_is_nonsequential() {
    let mut bus = bus();
    dma_registers(&mut bus, 3, 0x0200_0000, 0x0300_0000, 1, 0);
    let wait = WaitControl::default();
    let n = wait.cycles(0x0800_0002, Width::Half, Access::NonSequential);
    let s = wait.cycles(0x0800_0002, Width::Half, Access::Sequential);
    assert_ne!(n, s);
    bus.code_cycles(0x0800_0000, Width::Half, Access::NonSequential);
    assert_eq!(bus.dma_cycles(), 0);
    assert_eq!(
        bus.code_cycles(0x0800_0002, Width::Half, Access::Sequential),
        n
    );
    assert_ne!(bus.dma_cycles(), 0);
    assert_eq!(
        bus.code_cycles(0x0800_0004, Width::Half, Access::Sequential),
        s
    );
}

#[test]
fn siocnt_reads_the_busy_bit_at_the_access_cycle() {
    let siocnt = 0x0400_0128;
    let cycles = 8 * 8 + 6;
    for (idle, busy) in [(cycles - 1, 0x80), (cycles, 0)] {
        let mut bus = bus();
        bus.start_cpu_step();
        bus.cpu_write(siocnt, Width::Half, 0x0083);
        for _ in 0..idle {
            bus.cpu_idle();
        }
        assert_eq!(bus.cpu_read(siocnt, Width::Half) & 0x80, busy);
    }
}

fn prefetching_bus_with_dma(source: u32) -> Bus {
    let mut bus = bus();
    bus.write16(0x0400_0204, 0x4014);
    dma_registers(&mut bus, 3, source, 0x0300_0000, 4, 0x0400);
    bus.code_cycles(0x0800_0000, Width::Half, Access::NonSequential);
    assert_eq!(bus.dma_cycles(), 0);
    bus
}

#[test]
fn the_prefetch_buffer_keeps_filling_while_a_dma_runs_off_the_game_pak() {
    let mut bus = prefetching_bus_with_dma(0x0200_0000);
    assert_eq!(
        bus.code_cycles(0x0800_0002, Width::Half, Access::Sequential),
        1
    );
    assert_ne!(bus.dma_cycles(), 0);
}

#[test]
fn a_dma_from_the_game_pak_drops_the_prefetch_buffer() {
    let mut bus = prefetching_bus_with_dma(0x0800_0080);
    let n = bus
        .wait
        .cycles(0x0800_0002, Width::Half, Access::NonSequential);
    assert_eq!(
        bus.code_cycles(0x0800_0002, Width::Half, Access::Sequential),
        n
    );
    assert_ne!(bus.dma_cycles(), 0);
}

#[test]
fn a_held_instruction_skips_the_cycles_it_ran_before_the_halt() {
    let mut bus = bus();
    bus.start_cpu_step();
    bus.resume_held(1, 2);
    assert_eq!(
        bus.code_cycles(0x0300_0000, Width::Word, Access::Sequential),
        0
    );
    assert_eq!(
        bus.data_cycles(0x0300_0100, Width::Word, Access::NonSequential),
        0
    );
    assert_eq!(
        bus.code_cycles(0x0300_0004, Width::Word, Access::Sequential),
        1
    );
    assert_eq!(bus.step_cycles(), 2);
    assert_eq!(bus.end_held(), 0);
}

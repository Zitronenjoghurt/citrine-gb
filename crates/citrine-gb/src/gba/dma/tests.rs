use super::*;

const VBLANK: u16 = 1 << 12;
const HBLANK: u16 = 2 << 12;
const SPECIAL: u16 = 3 << 12;
const DEST_DECREMENT: u16 = 1 << 5;
const DEST_FIXED: u16 = 2 << 5;
const DEST_RELOAD: u16 = 3 << 5;
const SOURCE_DECREMENT: u16 = 1 << 7;
const SOURCE_FIXED: u16 = 2 << 7;

fn base(channel: usize) -> usize {
    CHANNEL_BYTES * channel
}

fn set_up(dma: &mut Dma, channel: usize, source: u32, dest: u32, count: u16) {
    let base = base(channel);
    dma.write(base + SOURCE_LOW, source as u16, 0xFFFF, 0);
    dma.write(base + SOURCE_HIGH, (source >> 16) as u16, 0xFFFF, 0);
    dma.write(base + DEST_LOW, dest as u16, 0xFFFF, 0);
    dma.write(base + DEST_HIGH, (dest >> 16) as u16, 0xFFFF, 0);
    dma.write(base + COUNT, count, 0xFFFF, 0);
}

fn enable(dma: &mut Dma, channel: usize, control: u16) {
    dma.write(base(channel) + CONTROL, control | ENABLE, 0xFFFF, 0);
    dma.activate(ACTIVATION_DELAY);
}

fn started(channel: usize, source: u32, dest: u32, count: u16, control: u16) -> Dma {
    let mut dma = Dma::default();
    set_up(&mut dma, channel, source, dest, count);
    enable(&mut dma, channel, control);
    dma
}

fn addresses(transfer: Transfer) -> Vec<(u32, u32)> {
    transfer.addresses().collect()
}

#[test]
fn only_control_reads_back() {
    let mut dma = Dma::default();
    set_up(&mut dma, 3, 0x0800_1234, 0x0300_5678, 9);
    dma.write(base(3) + CONTROL, 0x0240, 0xFFFF, 0);
    for offset in [SOURCE_LOW, SOURCE_HIGH, DEST_LOW, DEST_HIGH, COUNT] {
        assert_eq!(dma.read(base(3) + offset), 0, "offset {offset}");
    }
    assert_eq!(dma.read(base(3) + CONTROL), 0x0240);
}

#[test]
fn control_keeps_its_defined_bits_and_drq_only_on_dma3() {
    let mut dma = Dma::default();
    for channel in 0..CHANNELS {
        dma.write(base(channel) + CONTROL, 0x7FFF, 0xFFFF, 0);
    }
    assert_eq!(dma.control(0), 0x77E0);
    assert_eq!(dma.control(2), 0x77E0);
    assert_eq!(dma.control(3), 0x7FE0);
}

#[test]
fn immediate_starts_on_enable_and_clears_enable_when_done() {
    let mut dma = started(3, 0x0200_0000, 0x0300_0000, 4, 0);
    assert!(dma.active());
    let transfer = dma.next().unwrap();
    assert_eq!(transfer.units(), 4);
    assert_eq!(transfer.width(), Width::Half);
    dma.finish(transfer);
    assert!(!dma.active());
    assert_eq!(dma.control(3) & ENABLE, 0);
}

#[test]
fn immediate_ignores_repeat() {
    let mut dma = started(3, 0x0200_0000, 0x0300_0000, 4, REPEAT);
    let transfer = dma.next().unwrap();
    dma.finish(transfer);
    assert_eq!(dma.control(3) & ENABLE, 0);
}

#[test]
fn vblank_and_hblank_wait_for_their_trigger() {
    let mut dma = Dma::default();
    set_up(&mut dma, 1, 0x0200_0000, 0x0300_0000, 1);
    set_up(&mut dma, 2, 0x0200_0000, 0x0300_0000, 1);
    enable(&mut dma, 1, VBLANK);
    enable(&mut dma, 2, HBLANK);
    assert!(!dma.active());
    dma.trigger(Timing::HBlank);
    assert_eq!(dma.next().unwrap().channel(), 2);
    let transfer = dma.next().unwrap();
    dma.finish(transfer);
    assert!(!dma.active());
    dma.trigger(Timing::VBlank);
    assert_eq!(dma.next().unwrap().channel(), 1);
}

#[test]
fn a_trigger_does_not_start_a_disabled_channel() {
    let mut dma = Dma::default();
    dma.write(base(1) + CONTROL, VBLANK, 0xFFFF, 0);
    dma.trigger(Timing::VBlank);
    assert!(!dma.active());
}

#[test]
fn disabling_stops_a_waiting_channel() {
    let mut dma = started(1, 0x0200_0000, 0x0300_0000, 1, HBLANK);
    dma.trigger(Timing::HBlank);
    dma.write(base(1) + CONTROL, 0, 0xFFFF, 0);
    assert!(!dma.active());
}

#[test]
fn special_starts_channels_1_to_3_but_not_0() {
    for channel in 0..CHANNELS {
        let mut dma = started(channel, 0x0200_0000, 0x0300_0000, 1, SPECIAL);
        dma.trigger(Timing::VBlank);
        dma.trigger_special(channel);
        assert_eq!(dma.active(), channel > 0, "channel {channel}");
    }
}

#[test]
fn sound_fifo_moves_four_words_to_a_fixed_address() {
    for channel in [1, 2] {
        let mut dma = started(channel, 0x0200_0000, 0x0400_00A0, 100, SPECIAL | REPEAT);
        dma.trigger_special(channel);
        let transfer = dma.next().unwrap();
        assert_eq!(transfer.width(), Width::Word);
        assert_eq!(
            addresses(transfer),
            [0, 4, 8, 12].map(|step| (0x0200_0000 + step, 0x0400_00A0))
        );
        dma.finish(transfer);
        dma.trigger_special(channel);
        assert_eq!(dma.next().unwrap().source, 0x0200_0010);
    }
}

#[test]
fn video_capture_on_dma3_keeps_its_count() {
    let mut dma = started(3, 0x0200_0000, 0x0600_0000, 120, SPECIAL | REPEAT | WORD);
    dma.trigger_special(3);
    assert_eq!(dma.next().unwrap().units(), 120);
}

#[test]
fn count_zero_means_the_maximum_per_channel() {
    for (channel, max) in [(0, 0x4000), (1, 0x4000), (2, 0x4000), (3, 0x1_0000)] {
        let dma = started(channel, 0x0200_0000, 0x0300_0000, 0, 0);
        assert_eq!(dma.next().unwrap().units(), max, "channel {channel}");
    }
}

#[test]
fn count_keeps_14_bits_below_dma3() {
    let dma = started(0, 0x0200_0000, 0x0300_0000, 0xC005, 0);
    assert_eq!(dma.next().unwrap().units(), 5);
    let dma = started(3, 0x0200_0000, 0x0300_0000, 0xC005, 0);
    assert_eq!(dma.next().unwrap().units(), 0xC005);
}

#[test]
fn address_steps_for_halfwords() {
    let dma = started(
        3,
        0x0200_0010,
        0x0300_0010,
        3,
        DEST_DECREMENT | SOURCE_FIXED,
    );
    let pairs = addresses(dma.next().unwrap());
    assert_eq!(
        pairs,
        [
            (0x0200_0010, 0x0300_0010),
            (0x0200_0010, 0x0300_000E),
            (0x0200_0010, 0x0300_000C)
        ]
    );
}

#[test]
fn address_steps_for_words() {
    let dma = started(
        3,
        0x0200_0010,
        0x0300_0010,
        3,
        DEST_FIXED | SOURCE_DECREMENT | WORD,
    );
    let pairs = addresses(dma.next().unwrap());
    assert_eq!(
        pairs,
        [
            (0x0200_0010, 0x0300_0010),
            (0x0200_000C, 0x0300_0010),
            (0x0200_0008, 0x0300_0010)
        ]
    );
}

#[test]
fn source_control_3_steps_like_increment() {
    let dma = started(3, 0x0200_0000, 0x0300_0000, 2, 3 << 7);
    let pairs = addresses(dma.next().unwrap());
    assert_eq!(
        pairs,
        [(0x0200_0000, 0x0300_0000), (0x0200_0002, 0x0300_0002)]
    );
}

#[test]
fn addresses_align_to_the_unit_size() {
    let dma = started(3, 0x0200_0003, 0x0300_0007, 1, WORD);
    assert_eq!(addresses(dma.next().unwrap()), [(0x0200_0000, 0x0300_0004)]);
    let dma = started(3, 0x0200_0003, 0x0300_0007, 1, 0);
    assert_eq!(addresses(dma.next().unwrap()), [(0x0200_0002, 0x0300_0006)]);
}

#[test]
fn repeat_continues_from_where_the_last_block_ended() {
    let mut dma = started(1, 0x0200_0000, 0x0300_0000, 2, HBLANK | REPEAT);
    for block in 0..3 {
        dma.trigger(Timing::HBlank);
        let transfer = dma.next().unwrap();
        assert_eq!(
            addresses(transfer)[0],
            (0x0200_0000 + 4 * block, 0x0300_0000 + 4 * block)
        );
        dma.finish(transfer);
        assert_ne!(dma.control(1) & ENABLE, 0);
    }
}

#[test]
fn repeat_with_increment_reload_resets_only_the_destination() {
    let mut dma = started(
        1,
        0x0200_0000,
        0x0300_0000,
        2,
        HBLANK | REPEAT | DEST_RELOAD,
    );
    for block in 0..3 {
        dma.trigger(Timing::HBlank);
        let transfer = dma.next().unwrap();
        assert_eq!(
            addresses(transfer),
            [
                (0x0200_0000 + 4 * block, 0x0300_0000),
                (0x0200_0002 + 4 * block, 0x0300_0002)
            ]
        );
        dma.finish(transfer);
    }
}

#[test]
fn repeat_reloads_the_count_register_as_written_now() {
    let mut dma = started(1, 0x0200_0000, 0x0300_0000, 2, VBLANK | REPEAT);
    dma.write(base(1) + COUNT, 7, 0xFFFF, 0);
    dma.trigger(Timing::VBlank);
    let transfer = dma.next().unwrap();
    assert_eq!(transfer.units(), 2);
    dma.finish(transfer);
    dma.trigger(Timing::VBlank);
    assert_eq!(dma.next().unwrap().units(), 7);
}

#[test]
fn registers_latch_on_enable() {
    let mut dma = started(3, 0x0200_0000, 0x0300_0000, 2, 0);
    set_up(&mut dma, 3, 0x0200_1000, 0x0300_1000, 9);
    let transfer = dma.next().unwrap();
    assert_eq!(transfer.units(), 2);
    assert_eq!(addresses(transfer)[0], (0x0200_0000, 0x0300_0000));
}

#[test]
fn rewriting_control_while_enabled_does_not_relatch() {
    let mut dma = started(1, 0x0200_0000, 0x0300_0000, 2, VBLANK | REPEAT);
    dma.trigger(Timing::VBlank);
    let transfer = dma.next().unwrap();
    dma.finish(transfer);
    set_up(&mut dma, 1, 0x0200_1000, 0x0300_1000, 2);
    enable(&mut dma, 1, VBLANK | REPEAT);
    dma.trigger(Timing::VBlank);
    assert_eq!(
        addresses(dma.next().unwrap())[0],
        (0x0200_0004, 0x0300_0004)
    );
}

#[test]
fn irq_at_the_end_only_when_enabled() {
    let mut dma = started(2, 0x0200_0000, 0x0300_0000, 1, IRQ_ENABLE);
    let transfer = dma.next().unwrap();
    assert_eq!(dma.finish(transfer), Irq::DMA2);
    let mut dma = started(2, 0x0200_0000, 0x0300_0000, 1, 0);
    let transfer = dma.next().unwrap();
    assert_eq!(dma.finish(transfer), Irq::empty());
}

#[test]
fn address_masks_per_channel() {
    let rom = 0x0800_0010;
    let dma = started(0, rom, rom, 1, 0);
    assert_eq!(addresses(dma.next().unwrap()), [(0x0000_0010, 0x0000_0010)]);
    for channel in [1, 2] {
        let dma = started(channel, rom, rom, 1, 0);
        assert_eq!(addresses(dma.next().unwrap()), [(rom, 0x0000_0010)]);
    }
    let dma = started(3, 0xF800_0010, 0xFD00_0010, 1, 0);
    assert_eq!(addresses(dma.next().unwrap()), [(rom, 0x0D00_0010)]);
}

#[test]
fn addresses_wrap_inside_the_mask() {
    let dma = started(0, 0x07FF_FFFE, 0x0000_0000, 2, DEST_DECREMENT);
    assert_eq!(
        addresses(dma.next().unwrap()),
        [(0x07FF_FFFE, 0x0000_0000), (0x0000_0000, 0x07FF_FFFE)]
    );
}

#[test]
fn lower_channel_goes_first() {
    let mut dma = Dma::default();
    for channel in [3, 1] {
        set_up(&mut dma, channel, 0x0200_0000, 0x0300_0000, 1);
        enable(&mut dma, channel, HBLANK);
    }
    dma.trigger(Timing::HBlank);
    let first = dma.next().unwrap();
    assert_eq!(first.channel(), 1);
    dma.finish(first);
    let second = dma.next().unwrap();
    assert_eq!(second.channel(), 3);
    dma.finish(second);
    assert_eq!(dma.next(), None);
}

#[test]
fn cycles_are_two_n_plus_sequential_plus_two_internal() {
    let wait = WaitControl::default();
    let dma = started(3, 0x0300_0000, 0x0300_0100, 4, WORD);
    assert_eq!(dma.next().unwrap().cycles(wait), 2 + 4 + 4);
    let dma = started(3, 0x0200_0000, 0x0300_0000, 4, 0);
    assert_eq!(dma.next().unwrap().cycles(wait), 2 + 4 * 3 + 4);
    let dma = started(3, 0x0200_0000, 0x0300_0000, 4, WORD);
    assert_eq!(dma.next().unwrap().cycles(wait), 2 + 4 * 6 + 4);
}

#[test]
fn rom_source_pays_first_and_second_access_waits() {
    let wait = WaitControl::default();
    let first = wait.cycles(0x0800_0000, Width::Half, Access::NonSequential);
    let second = wait.cycles(0x0800_0002, Width::Half, Access::Sequential);
    let dma = started(3, 0x0800_0000, 0x0300_0000, 3, 0);
    assert_eq!(dma.next().unwrap().cycles(wait), 2 + first + 2 * second + 3);
}

#[test]
fn rom_to_rom_pays_four_internal_cycles() {
    let wait = WaitControl::default();
    let dma = started(3, 0x0800_0000, 0x0A00_0000, 1, 0);
    let access = wait.cycles(0x0800_0000, Width::Half, Access::NonSequential)
        + wait.cycles(0x0A00_0000, Width::Half, Access::NonSequential);
    assert_eq!(dma.next().unwrap().cycles(wait), 4 + access);
}

#[test]
fn an_immediate_transfer_activates_two_cycles_after_its_enable_write() {
    let mut dma = Dma::default();
    set_up(&mut dma, 3, 0x0200_0000, 0x0300_0000, 1);
    dma.write(base(3) + CONTROL, ENABLE, 0xFFFF, 5);
    assert_eq!(dma.due(), Some(7));
    assert!(!dma.activate(6));
    assert!(dma.activate(7));
    assert_eq!(dma.due(), None);
}

#[test]
fn a_blank_request_activates_two_cycles_later_across_steps() {
    let mut dma = Dma::default();
    set_up(&mut dma, 0, 0x0200_0000, 0x0300_0000, 1);
    dma.write(base(0) + CONTROL, ENABLE | HBLANK, 0xFFFF, 0);
    assert!(dma.waits_on_video());
    dma.request(Timing::HBlank, 10);
    dma.request(Timing::HBlank, 11);
    dma.elapse(11);
    assert_eq!(dma.due(), Some(1));
    assert!(dma.active());
    assert!(!dma.running());
    assert!(dma.activate(1));
}

#[test]
fn disabling_a_scheduled_channel_drops_its_activation() {
    let mut dma = Dma::default();
    set_up(&mut dma, 3, 0x0200_0000, 0x0300_0000, 1);
    dma.write(base(3) + CONTROL, ENABLE, 0xFFFF, 0);
    dma.write(base(3) + CONTROL, 0, 0xFFFF, 1);
    assert!(!dma.active());
    assert!(!dma.activate(10));
}

fn read_cycles(transfer: Transfer, wait: WaitControl) -> Vec<u32> {
    transfer
        .addresses()
        .enumerate()
        .map(|(unit, (source, dest))| transfer.unit_cycles(wait, unit, source, dest).0)
        .collect()
}

fn rom_waits() -> WaitControl {
    let mut wait = WaitControl::default();
    wait.write(0x0014, 0xFFFF);
    wait
}

#[test]
fn a_fixed_rom_source_at_a_page_end_reads_every_unit_nonsequential() {
    let wait = rom_waits();
    let dma = started(3, 0x0801_FFFC, 0x0300_0000, 3, WORD | SOURCE_FIXED);
    let n = wait.cycles(0x0801_FFFC, Width::Word, Access::NonSequential);
    assert_eq!(read_cycles(dma.next().unwrap(), wait), [n, n, n]);
}

#[test]
fn an_incrementing_rom_source_reads_the_next_page_nonsequential() {
    let wait = rom_waits();
    let dma = started(3, 0x0801_FFF8, 0x0300_0000, 3, WORD);
    let n = wait.cycles(0x0801_FFF8, Width::Word, Access::NonSequential);
    let s = wait.cycles(0x0801_FFF8, Width::Word, Access::Sequential);
    assert_eq!(read_cycles(dma.next().unwrap(), wait), [n, s, n]);
}

#[test]
fn a_decrementing_rom_source_reads_nonsequential_after_a_page_end() {
    let wait = rom_waits();
    let dma = started(3, 0x0802_0000, 0x0300_0000, 3, SOURCE_DECREMENT);
    let n = wait.cycles(0x0802_0000, Width::Half, Access::NonSequential);
    let s = wait.cycles(0x0801_FFFE, Width::Half, Access::Sequential);
    assert_eq!(read_cycles(dma.next().unwrap(), wait), [n, s, n]);
}

#[test]
fn a_paused_channel_resumes_with_the_units_it_has_left() {
    let mut dma = started(3, 0x0200_0000, 0x0300_0000, 4, 0);
    dma.pause(dma.next().unwrap(), 3);
    assert!(dma.running());
    let transfer = dma.next().unwrap();
    assert_eq!(addresses(transfer), [(0x0200_0006, 0x0300_0006)]);
    dma.finish(transfer);
    assert!(!dma.running());
    assert_eq!(dma.control(3) & ENABLE, 0);
}

#[test]
fn a_paused_sound_fifo_dma_moves_only_its_remaining_words() {
    let mut dma = started(1, 0x0200_0000, 0x0400_00A0, 0, SPECIAL | REPEAT | WORD);
    dma.trigger_special(1);
    dma.pause(dma.next().unwrap(), 1);
    let transfer = dma.next().unwrap();
    assert_eq!(transfer.units(), 3);
    dma.finish(transfer);
    dma.trigger_special(1);
    assert_eq!(dma.next().unwrap().units(), 4);
}

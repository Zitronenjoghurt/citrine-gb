use super::*;
use crate::gba::bus::Bus;
use crate::gba::timer::START_DELAY;

const SOUND1CNT_H: u32 = 0x0400_0062;
const SOUND1CNT_X: u32 = 0x0400_0064;
const SOUND2CNT_L: u32 = 0x0400_0068;
const SOUND2CNT_H: u32 = 0x0400_006C;
const SOUND3CNT_L: u32 = 0x0400_0070;
const SOUND3CNT_H: u32 = 0x0400_0072;
const SOUND3CNT_X: u32 = 0x0400_0074;
const SOUND4CNT_L: u32 = 0x0400_0078;
const SOUND4CNT_H: u32 = 0x0400_007C;
const SOUNDCNT_L: u32 = 0x0400_0080;
const SOUNDCNT_H: u32 = 0x0400_0082;
const SOUNDCNT_X: u32 = 0x0400_0084;
const SOUNDBIAS: u32 = 0x0400_0088;
const WAVE_RAM: u32 = 0x0400_0090;
const FIFO_A: u32 = 0x0400_00A0;
const FIFO_B: u32 = 0x0400_00A4;
const DMA1SAD: u32 = 0x0400_00BC;
const DMA1DAD: u32 = 0x0400_00C0;
const DMA1CNT_H: u32 = 0x0400_00C6;
const TM0CNT_L: u32 = 0x0400_0100;
const TM0CNT_H: u32 = 0x0400_0102;
const TM1CNT_L: u32 = 0x0400_0104;
const TM1CNT_H: u32 = 0x0400_0106;
const EWRAM: u32 = 0x0200_0000;
const SILENCE: u16 = 0x200;
const QUARTER_SECOND: u32 = CLOCK_RATE / 4;
const FIFO_DMA: u16 = 0xB640;
const TIMER_ON: u16 = 0x0080;

fn bus() -> Bus {
    Bus::new(&[])
}

fn sound_on(bus: &mut Bus, control: u16) {
    bus.write16(SOUNDCNT_X, 0x0080);
    bus.write16(SOUNDCNT_L, 0xFF77);
    bus.write16(SOUNDCNT_H, control);
}

fn run(bus: &mut Bus, cycles: u32) {
    let mut left = cycles;
    while left > 0 {
        let chunk = left.min(64);
        bus.step(chunk);
        if bus.dma_active() {
            bus.run_dma();
        }
        left -= chunk;
    }
}

fn record(bus: &mut Bus, cycles: u32) -> Vec<Dac> {
    let every = bus.sound().sample_cycles();
    (0..cycles / every)
        .map(|_| {
            run(bus, every);
            bus.sound().dac()
        })
        .collect()
}

fn lefts(samples: &[Dac]) -> Vec<u16> {
    samples.iter().map(|dac| dac.left).collect()
}

fn rising_edges(levels: &[u16]) -> usize {
    levels.windows(2).filter(|pair| pair[1] > pair[0]).count()
}

fn changes(levels: &[u16]) -> Vec<u16> {
    let mut changes = levels.to_vec();
    changes.dedup();
    changes
}

fn start_timer(bus: &mut Bus, timer: u32, period: u16) {
    let (reload, control) = if timer == 0 {
        (TM0CNT_L, TM0CNT_H)
    } else {
        (TM1CNT_L, TM1CNT_H)
    };
    bus.write16(reload, period.wrapping_neg());
    bus.write16(control, TIMER_ON);
}

fn push(bus: &mut Bus, fifo: u32, samples: [i8; 4]) {
    bus.write32(fifo, u32::from_le_bytes(samples.map(|sample| sample as u8)));
}

fn fill_wave_bank(bus: &mut Bus, bytes: [u8; 16]) {
    for (index, pair) in bytes.chunks(2).enumerate() {
        bus.write16(
            WAVE_RAM + 2 * index as u32,
            u16::from_le_bytes([pair[0], pair[1]]),
        );
    }
}

#[test]
fn sound1_plays_a_square_at_the_period_its_registers_set() {
    let mut bus = bus();
    sound_on(&mut bus, 0x0002);
    bus.write16(SOUND1CNT_H, 0xF080);
    bus.write16(SOUND1CNT_X, 0x8000 | 1920);

    let levels = lefts(&record(&mut bus, QUARTER_SECOND));

    assert!(
        (255..=257).contains(&rising_edges(&levels)),
        "{}",
        rising_edges(&levels)
    );
    let mut seen = levels[16..].to_vec();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen, [SILENCE - 120, SILENCE + 120]);
    assert_eq!(bus.read16(SOUNDCNT_X), 0x0081);
}

#[test]
fn psg_registers_read_back_without_their_write_only_bits() {
    let mut bus = bus();
    sound_on(&mut bus, 0);
    bus.write16(SOUND1CNT_H, 0xF0BF);
    bus.write16(SOUND1CNT_X, 0x4780);
    bus.write16(SOUND2CNT_L, 0xA0FF);
    bus.write16(SOUND2CNT_H, 0xC123);
    bus.write16(SOUNDCNT_H, 0xFFFF);

    assert_eq!(bus.read16(SOUND1CNT_H), 0xF080);
    assert_eq!(bus.read16(SOUND1CNT_X), 0x4000);
    assert_eq!(bus.read16(SOUND2CNT_L), 0xA0C0);
    assert_eq!(bus.read16(SOUND2CNT_H), 0x4000);
    assert_eq!(bus.read16(SOUNDCNT_L), 0xFF77);
    assert_eq!(bus.read16(SOUNDCNT_H), 0x770F);
    assert_eq!(bus.read16(SOUNDCNT_X), 0x0082);
    assert_eq!(bus.read16(SOUNDBIAS), SILENCE);
}

#[test]
fn master_off_clears_the_psg_registers_and_ignores_writes_to_them() {
    let mut bus = bus();
    sound_on(&mut bus, 0x0002);
    bus.write16(SOUND1CNT_H, 0xF080);

    bus.write16(SOUNDCNT_X, 0);
    bus.write16(SOUNDCNT_L, 0x1234);

    assert_eq!(bus.read16(SOUNDCNT_L), 0);
    assert_eq!(bus.read16(SOUND1CNT_H), 0);
    assert_eq!(bus.read16(SOUNDCNT_H), 0x0002);
    bus.write16(SOUNDCNT_X, 0x0080);
    bus.write16(SOUNDCNT_L, 0x1234);
    assert_eq!(bus.read16(SOUNDCNT_L), 0x1234 & 0xFF77);
}

#[test]
fn master_off_empties_the_fifos_and_drops_writes_to_them() {
    let mut bus = bus();
    sound_on(&mut bus, 0);
    bus.write32(FIFO_A, 0x0403_0201);
    bus.write32(FIFO_B, 0x0403_0201);

    bus.write16(SOUNDCNT_X, 0);
    assert_eq!(bus.sound().fifo_len(0), 0);
    assert_eq!(bus.sound().fifo_len(1), 0);
    bus.write32(FIFO_A, 0x0403_0201);
    assert_eq!(bus.sound().fifo_len(0), 0);

    bus.write16(SOUNDCNT_X, 0x0080);
    bus.write32(FIFO_A, 0x0403_0201);
    assert_eq!(bus.sound().fifo_len(0), 4);
}

#[test]
fn the_cpu_sees_the_wave_bank_that_is_not_playing() {
    let mut bus = bus();
    sound_on(&mut bus, 0x0002);
    bus.write16(SOUND3CNT_L, 0x0000);
    fill_wave_bank(
        &mut bus,
        [
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
    );
    assert_eq!(bus.read16(WAVE_RAM), 0xFFFF);

    bus.write16(SOUND3CNT_L, 0x00C0);
    assert_eq!(bus.read16(WAVE_RAM), 0x0000);
    bus.write16(SOUND3CNT_H, 0x2000);
    bus.write16(SOUND3CNT_X, 0x8000 | 1984);

    let levels = lefts(&record(&mut bus, QUARTER_SECOND));

    assert!(
        (255..=257).contains(&rising_edges(&levels)),
        "{}",
        rising_edges(&levels)
    );
    assert_eq!(levels.iter().max(), Some(&(SILENCE + 120)));
    assert_eq!(levels.iter().min(), Some(&(SILENCE - 120)));
}

#[test]
fn wave_64_digit_mode_plays_both_banks_in_turn() {
    let mut bus = bus();
    sound_on(&mut bus, 0x0002);
    bus.write16(SOUND3CNT_L, 0x0040);
    fill_wave_bank(&mut bus, [0xFF; 16]);
    bus.write16(SOUND3CNT_H, 0x2000);

    bus.write16(SOUND3CNT_L, 0x0080);
    bus.write16(SOUND3CNT_X, 0x8000 | 1984);
    let one_bank = lefts(&record(&mut bus, QUARTER_SECOND));

    bus.write16(SOUND3CNT_L, 0x00A0);
    bus.write16(SOUND3CNT_X, 0x8000 | 1984);
    let two_banks = lefts(&record(&mut bus, QUARTER_SECOND));

    assert_eq!(changes(&one_bank[16..]), [SILENCE + 120]);
    assert!(
        (127..=129).contains(&rising_edges(&two_banks)),
        "{}",
        rising_edges(&two_banks)
    );
}

#[test]
fn wave_volume_follows_its_level_bits_and_the_forced_75_percent() {
    let mut bus = bus();
    sound_on(&mut bus, 0x0002);
    bus.write16(SOUND3CNT_L, 0x0040);
    fill_wave_bank(&mut bus, [0xFF; 16]);
    bus.write16(SOUND3CNT_L, 0x0080);
    bus.write16(SOUND3CNT_X, 0x8000 | 1984);

    let mut level = |volume: u16| {
        bus.write16(SOUND3CNT_H, volume);
        run(&mut bus, 4096);
        bus.sound().dac().left
    };

    assert_eq!(level(0x2000), SILENCE + 120);
    assert_eq!(level(0x4000), SILENCE + 56);
    assert_eq!(level(0x6000), SILENCE + 24);
    assert_eq!(level(0x8000), SILENCE + 88);
    assert_eq!(level(0x0000), SILENCE);
}

#[test]
fn soundcnt_h_scales_the_psg_to_a_quarter_half_or_full() {
    let mut bus = bus();
    sound_on(&mut bus, 0x0000);
    bus.write16(SOUND2CNT_L, 0xF080);
    bus.write16(SOUND2CNT_H, 0x8000 | 1920);

    let mut peak = |control: u16| {
        bus.write16(SOUNDCNT_H, control);
        let levels = lefts(&record(&mut bus, CLOCK_RATE / 64));
        *levels.iter().max().unwrap() - SILENCE
    };

    assert_eq!(peak(0x0000), 30);
    assert_eq!(peak(0x0001), 60);
    assert_eq!(peak(0x0002), 120);
}

#[test]
fn soundcnt_l_master_volume_and_routing_set_each_side() {
    let mut bus = bus();
    sound_on(&mut bus, 0x0002);
    bus.write16(SOUNDCNT_L, 0x2073);
    bus.write16(SOUND2CNT_L, 0xF080);
    bus.write16(SOUND2CNT_H, 0x8000 | 1920);

    let samples = record(&mut bus, CLOCK_RATE / 64);

    assert_eq!(
        samples.iter().map(|dac| dac.left).max(),
        Some(SILENCE + 120)
    );
    assert_eq!(samples.iter().map(|dac| dac.right).max(), Some(SILENCE));
    bus.write16(SOUNDCNT_L, 0x2273);
    let samples = record(&mut bus, CLOCK_RATE / 64);
    assert_eq!(
        samples.iter().map(|dac| dac.right).max(),
        Some(SILENCE + 60)
    );
}

#[test]
fn a_fifo_plays_one_sample_per_overflow_of_its_timer() {
    let mut bus = bus();
    sound_on(&mut bus, 0x0304);
    push(&mut bus, FIFO_A, [10, -20, 30, 40]);
    assert_eq!(bus.sound().fifo_len(0), 4);

    start_timer(&mut bus, 0, 512);
    let levels = lefts(&record(&mut bus, 512 * 8));

    assert_eq!(
        changes(&levels),
        [
            SILENCE,
            SILENCE + 40,
            SILENCE - 80,
            SILENCE + 120,
            SILENCE + 160
        ]
    );
    assert_eq!(bus.sound().fifo_len(0), 0);
}

#[test]
fn each_fifo_has_its_own_timer_volume_and_sides() {
    let mut bus = bus();
    sound_on(&mut bus, 0x6108);
    push(&mut bus, FIFO_A, [100; 4]);
    push(&mut bus, FIFO_B, [-50; 4]);

    start_timer(&mut bus, 1, 256);
    run(&mut bus, 1024);
    assert_eq!(
        bus.sound().dac(),
        Dac {
            left: SILENCE - 200,
            right: SILENCE
        }
    );
    assert_eq!(bus.sound().fifo_len(0), 4);

    start_timer(&mut bus, 0, 256);
    run(&mut bus, 1024);
    assert_eq!(
        bus.sound().dac(),
        Dac {
            left: SILENCE - 200,
            right: SILENCE + 200
        }
    );
}

#[test]
fn a_fifo_asks_its_dma_for_more_once_three_words_wait_behind_the_playing_one() {
    let mut bus = bus();
    sound_on(&mut bus, 0x0304);
    for word in 0..8 {
        bus.write32(EWRAM + 4 * word, 0x7F7F_7F7F);
    }
    bus.write32(DMA1SAD, EWRAM);
    bus.write32(DMA1DAD, FIFO_A);
    bus.write16(DMA1CNT_H, FIFO_DMA);
    for _ in 0..5 {
        push(&mut bus, FIFO_A, [1; 4]);
    }
    start_timer(&mut bus, 0, 256);

    bus.step(256 + START_DELAY);
    assert_eq!(bus.sound().fifo_len(0), 19);
    assert!(!bus.dma_active());

    bus.step(4 * 256);
    assert_eq!(bus.sound().fifo_len(0), 15);
    assert!(!bus.dma_active());

    bus.step(256);
    assert_eq!(bus.sound().fifo_len(0), 14);
    assert!(bus.dma_active());
    bus.run_dma();
    assert_eq!(bus.sound().fifo_len(0), 30);
}

#[test]
fn an_empty_fifo_asks_on_the_first_overflow_and_again_on_the_third() {
    let mut bus = bus();
    sound_on(&mut bus, 0x0304);
    bus.write32(DMA1SAD, EWRAM);
    bus.write32(DMA1DAD, FIFO_A);
    bus.write16(DMA1CNT_H, FIFO_DMA);
    start_timer(&mut bus, 0, 256);

    bus.step(256 + START_DELAY);
    assert!(bus.dma_active());
    bus.run_dma();
    assert_eq!(bus.sound().fifo_len(0), 16);

    bus.step(256);
    assert_eq!(bus.sound().fifo_len(0), 15);
    assert!(!bus.dma_active());

    bus.step(256);
    assert_eq!(bus.sound().fifo_len(0), 14);
    assert!(bus.dma_active());
}

#[test]
fn a_fifo_request_starts_only_the_dma_aimed_at_that_fifo() {
    let mut bus = bus();
    sound_on(&mut bus, 0x4304);
    bus.write32(DMA1SAD, EWRAM);
    bus.write32(DMA1DAD, FIFO_B);
    bus.write16(DMA1CNT_H, FIFO_DMA);
    push(&mut bus, FIFO_A, [1; 4]);
    start_timer(&mut bus, 0, 256);

    bus.step(256 + START_DELAY);

    assert_eq!(bus.sound().fifo_len(0), 3);
    assert!(!bus.dma_active());
}

#[test]
fn the_reset_bit_empties_a_fifo_and_reads_back_as_zero() {
    let mut bus = bus();
    sound_on(&mut bus, 0x0304);
    push(&mut bus, FIFO_A, [1; 4]);
    push(&mut bus, FIFO_B, [1; 4]);

    bus.write16(SOUNDCNT_H, 0x0B04);

    assert_eq!(bus.sound().fifo_len(0), 0);
    assert_eq!(bus.sound().fifo_len(1), 4);
    assert_eq!(bus.read16(SOUNDCNT_H), 0x0304);
}

#[test]
fn soundbias_sets_the_silent_level_and_the_output_clips_to_10_bits() {
    let mut bus = bus();
    sound_on(&mut bus, 0x0304);
    start_timer(&mut bus, 0, 256);

    bus.write16(SOUNDBIAS, 0x0100);
    run(&mut bus, 1024);
    assert_eq!(bus.sound().dac().left, 0x100);

    push(&mut bus, FIFO_A, [-128; 4]);
    run(&mut bus, 1024);
    assert_eq!(bus.sound().dac().left, 0);

    bus.write16(SOUNDBIAS, 0x03FE);
    push(&mut bus, FIFO_A, [127; 4]);
    run(&mut bus, 1024);
    assert_eq!(bus.sound().dac().left, 0x3FE);
}

#[test]
fn soundbias_resolution_sets_the_sampling_cycle_and_drops_low_bits() {
    let mut bus = bus();
    sound_on(&mut bus, 0x0304);
    let cycles: Vec<u32> = (0..4)
        .map(|resolution| {
            bus.write16(SOUNDBIAS, resolution << 14 | SILENCE);
            bus.sound().sample_cycles()
        })
        .collect();
    assert_eq!(cycles, [512, 256, 128, 64]);

    push(&mut bus, FIFO_A, [7; 4]);
    start_timer(&mut bus, 0, 64);
    run(&mut bus, 256);
    assert_eq!(bus.sound().dac().left, SILENCE + 16);

    bus.write16(SOUNDBIAS, 0x4000 | SILENCE);
    run(&mut bus, 512);
    assert_eq!(bus.sound().dac().left, SILENCE + 28);
}

#[test]
fn take_audio_gives_stereo_floats_at_the_chosen_rate() {
    let mut bus = bus();
    bus.sound_mut().set_sample_rate(48_000);
    sound_on(&mut bus, 0x0002);
    bus.write16(SOUND1CNT_H, 0xF080);
    bus.write16(SOUND1CNT_X, 0x8000 | 1920);

    let mut audio = Vec::new();
    for _ in 0..4 {
        run(&mut bus, QUARTER_SECOND);
        bus.sound_mut().take_audio(&mut audio);
    }

    assert!((95_800..=96_200).contains(&audio.len()), "{}", audio.len());
    let peak = audio
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
    assert!((0.2..=0.5).contains(&peak), "{peak}");
}

#[test]
fn undrained_audio_stays_bounded() {
    let mut bus = bus();
    bus.sound_mut().set_sample_rate(8_000);
    sound_on(&mut bus, 0x0002);
    bus.write16(SOUND1CNT_H, 0xF080);
    bus.write16(SOUND1CNT_X, 0x8000 | 1920);

    run(&mut bus, 3 * CLOCK_RATE);

    let mut audio = Vec::new();
    bus.sound_mut().take_audio(&mut audio);
    assert!(audio.len() <= 16_000, "{}", audio.len());
}

#[test]
fn a_muted_channel_leaves_the_mix() {
    let mut bus = bus();
    sound_on(&mut bus, 0x0306);
    bus.write16(SOUND1CNT_H, 0xF080);
    bus.write16(SOUND1CNT_X, 0x8000 | 1920);
    push(&mut bus, FIFO_A, [50; 4]);
    start_timer(&mut bus, 0, 256);

    bus.sound_mut().set_channel_enabled(0, false);
    let levels = lefts(&record(&mut bus, CLOCK_RATE / 64));
    assert_eq!(changes(&levels[4..]), [SILENCE + 200]);

    bus.sound_mut().set_channel_enabled(4, false);
    run(&mut bus, 1024);
    assert_eq!(bus.sound().dac().left, SILENCE);
    assert_eq!(CHANNEL_NAMES.len(), 6);
}

fn play_alone(channel: usize) -> Bus {
    let mut bus = bus();
    match channel {
        0 => {
            sound_on(&mut bus, 0x0002);
            bus.write16(SOUND1CNT_H, 0xF080);
            bus.write16(SOUND1CNT_X, 0x8000 | 1920);
        }
        1 => {
            sound_on(&mut bus, 0x0002);
            bus.write16(SOUND2CNT_L, 0xF080);
            bus.write16(SOUND2CNT_H, 0x8000 | 1920);
        }
        2 => {
            sound_on(&mut bus, 0x0002);
            bus.write16(SOUND3CNT_L, 0x0040);
            fill_wave_bank(&mut bus, [0xF0; 16]);
            bus.write16(SOUND3CNT_H, 0x2000);
            bus.write16(SOUND3CNT_L, 0x0080);
            bus.write16(SOUND3CNT_X, 0x8000 | 1984);
        }
        3 => {
            sound_on(&mut bus, 0x0002);
            bus.write16(SOUND4CNT_L, 0xF000);
            bus.write16(SOUND4CNT_H, 0x8000);
        }
        4 => {
            sound_on(&mut bus, 0x0304);
            push(&mut bus, FIFO_A, [50; 4]);
            start_timer(&mut bus, 0, 256);
        }
        _ => {
            sound_on(&mut bus, 0x3008);
            push(&mut bus, FIFO_B, [50; 4]);
            start_timer(&mut bus, 0, 256);
        }
    }
    bus
}

#[test]
fn each_mute_silences_its_own_channel() {
    for (channel, name) in CHANNEL_NAMES.iter().enumerate() {
        let mut playing = play_alone(channel);
        let heard = lefts(&record(&mut playing, CLOCK_RATE / 64));
        assert!(heard.iter().any(|&level| level != SILENCE), "{name} plays");

        let mut muted = play_alone(channel);
        muted.sound_mut().set_channel_enabled(channel, false);
        let heard = lefts(&record(&mut muted, CLOCK_RATE / 64));
        assert!(
            heard.iter().all(|&level| level == SILENCE),
            "{name} is silent"
        );
    }
}

#[test]
#[ignore = "writes a listening file to CITRINE_GBA_WAV"]
fn render_listening_wav() {
    let path = std::env::var("CITRINE_GBA_WAV").expect("CITRINE_GBA_WAV");
    let rate = 48_000;
    let mut bus = bus();
    bus.sound_mut().set_sample_rate(rate);
    sound_on(&mut bus, 0x0306);
    bus.write16(SOUND2CNT_L, 0x0080);

    let fifo_period = 512;
    let half_wave = 37;
    for index in 0..CLOCK_RATE / fifo_period * 2 {
        let level = if (index / half_wave) % 2 == 0 {
            48
        } else {
            -48
        };
        bus.write8(EWRAM + index, level as u8);
    }
    bus.write32(DMA1SAD, EWRAM);
    bus.write32(DMA1DAD, FIFO_A);
    bus.write16(DMA1CNT_H, FIFO_DMA);

    let notes = [1546, 1650, 1714, 1797];
    let note_cycles = CLOCK_RATE / 4;
    let mut audio = Vec::new();
    for second in 0..3 {
        if second == 1 {
            start_timer(&mut bus, 0, fifo_period as u16);
        }
        for period in notes {
            if second != 1 {
                bus.write16(SOUND1CNT_H, 0xF3C0);
                bus.write16(SOUND1CNT_X, 0x8000 | period);
            }
            run(&mut bus, note_cycles);
            bus.sound_mut().take_audio(&mut audio);
        }
    }

    std::fs::write(path, wav_bytes(&audio, rate)).expect("write wav");
}

fn wav_bytes(audio: &[f32], rate: u32) -> Vec<u8> {
    let data_len = (audio.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 4).to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for sample in audio {
        let value = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[cfg(feature = "debug")]
#[test]
fn a_silent_direct_sound_channel_plots_on_the_midline_and_an_unrouted_one_as_off() {
    let mut bus = bus();
    sound_on(&mut bus, 0x0304);
    bus.sound_mut().record_taps(true);
    run(&mut bus, 4096);

    let mut debugger = crate::debug::Debugger::new();
    for levels in bus.sound_mut().take_taps() {
        debugger.push_waves(&levels);
    }
    let waves = debugger.waves();
    assert!(!waves[4].is_empty());
    assert!(waves[4].iter().all(|&level| (level - 0.5).abs() < 1.0e-6));
    assert!(waves[5].iter().all(|&level| level == 0.0));
}

fn fifo_audio_in_steps_of(step: u32) -> Vec<f32> {
    let mut bus = bus();
    bus.sound_mut().set_sample_rate(48_000);
    sound_on(&mut bus, 0x0304);
    for word in 0..7_i8 {
        push(
            &mut bus,
            FIFO_A,
            [word * 9, -word * 7, 60 - word * 5, word * 3 - 40],
        );
    }
    start_timer(&mut bus, 0, 1254);
    let mut left = 1254 * 30;
    while left > 0 {
        let chunk = left.min(step);
        bus.step(chunk);
        left -= chunk;
    }
    let mut audio = Vec::new();
    bus.sound_mut().take_audio(&mut audio);
    audio
}

#[test]
fn a_long_cpu_step_plays_every_fifo_sample_at_its_overflow() {
    let fine = fifo_audio_in_steps_of(16);
    assert_eq!(fifo_audio_in_steps_of(64), fine);
    assert_eq!(fifo_audio_in_steps_of(3_000), fine);
    assert_eq!(fifo_audio_in_steps_of(7_000), fine);
}

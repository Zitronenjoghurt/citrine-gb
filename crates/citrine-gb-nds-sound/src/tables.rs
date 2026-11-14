use std::sync::LazyLock;

pub const SILENT_DB: i32 = -723;
const PITCH_STEPS: i32 = 768;
const TIMER_MIN: u32 = 0x10;
const TIMER_MAX: u32 = 0xFFFF;
const DIVIDERS: [(i32, u8, f64); 4] = [
    (-60, 0, 1.0),
    (-120, 1, 2.0),
    (-240, 2, 4.0),
    (i32::MIN, 3, 16.0),
];

static DECIBELS: LazyLock<[i16; 128]> = LazyLock::new(|| {
    std::array::from_fn(|x| match x {
        0 => SILENT_DB as i16,
        1 => SILENT_DB as i16 + 1,
        x => (400.0 * (x as f64 / 127.0).log10()).round() as i16,
    })
});

static PITCHES: LazyLock<Vec<u32>> = LazyLock::new(|| {
    (0..PITCH_STEPS)
        .map(|p| (65536.0 * ((f64::from(p) / f64::from(PITCH_STEPS)).exp2() - 1.0)).round() as u32)
        .collect()
});

static VOLUMES: LazyLock<Vec<(u8, u8)>> = LazyLock::new(|| {
    (SILENT_DB..=0)
        .map(|db| {
            let &(_, divider, gain) = DIVIDERS
                .iter()
                .find(|&&(floor, ..)| db >= floor)
                .unwrap_or(&DIVIDERS[3]);
            let level = 127.0 * gain * 10f64.powf(f64::from(db) / 200.0);
            (level.round().min(127.0) as u8, divider)
        })
        .collect()
});

pub fn decibels(value: u8) -> i32 {
    i32::from(DECIBELS[usize::from(value.min(127))])
}

pub fn channel_volume(db: i32) -> (u8, u8) {
    let index = (db.clamp(SILENT_DB, 0) - SILENT_DB) as usize;
    VOLUMES[index]
}

pub fn adjust_timer(timer: u16, pitch: i32) -> u16 {
    let mut pitch = -pitch;
    let mut shift = 0i32;
    while pitch < 0 {
        shift -= 1;
        pitch += PITCH_STEPS;
    }
    while pitch >= PITCH_STEPS {
        shift += 1;
        pitch -= PITCH_STEPS;
    }
    let scaled = u64::from(timer) * u64::from(PITCHES[pitch as usize] + 0x10000);
    let shift = shift - 16;
    let value = if shift <= 0 {
        scaled >> -shift
    } else if shift < 32 {
        scaled << shift
    } else {
        u64::from(TIMER_MAX)
    };
    value.clamp(u64::from(TIMER_MIN), u64::from(TIMER_MAX)) as u16
}

pub fn sine(index: u8) -> i32 {
    let quarter =
        |i: u8| (127.0 * (f64::from(i) * std::f64::consts::PI / 64.0).sin()).round() as i32;
    let i = index & 0x7F;
    match i {
        0..=32 => quarter(i),
        33..=64 => quarter(64 - i),
        65..=96 => -quarter(i - 64),
        _ => -quarter(128 - i),
    }
}

pub fn attack_rate(attack: u8) -> i32 {
    const STEEP: [i32; 19] = [
        0x00, 0x01, 0x05, 0x0E, 0x1A, 0x26, 0x33, 0x3F, 0x49, 0x54, 0x5C, 0x64, 0x6D, 0x74, 0x7B,
        0x7F, 0x84, 0x89, 0x8F,
    ];
    let attack = attack.min(127);
    if attack >= 0x6D {
        STEEP[usize::from(0x7F - attack)]
    } else {
        0xFF - i32::from(attack)
    }
}

pub fn fall_rate(rate: u8) -> i32 {
    match rate.min(127) {
        0x7F => 0xFFFF,
        0x7E => 0x3C00,
        r if r < 0x32 => ((i32::from(r) << 1) + 1) & 0xFFFF,
        r => (0x1E00 / (0x7E - i32::from(r))) & 0xFFFF,
    }
}

pub fn sustain_level(sustain: u8) -> i32 {
    decibels(sustain) << 7
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decibels_match_the_published_ends_of_the_sdk_table() {
        let first: Vec<i32> = (0..8).map(decibels).collect();
        assert_eq!(first, [-723, -722, -721, -651, -601, -562, -530, -503]);
        let last: Vec<i32> = (120..128).map(decibels).collect();
        assert_eq!(last, [-10, -8, -7, -6, -4, -3, -1, 0]);
    }

    #[test]
    fn an_octave_halves_or_doubles_the_timer() {
        assert_eq!(adjust_timer(1000, 0), 1000);
        assert_eq!(adjust_timer(1000, 768), 500);
        assert_eq!(adjust_timer(1000, -768), 2000);
        assert_eq!(adjust_timer(1000, 64), 943);
        assert_eq!(adjust_timer(0xF000, -768), 0xFFFF);
        assert_eq!(adjust_timer(16, 768 * 2), 0x10);
    }

    #[test]
    fn full_level_is_full_volume_and_quiet_levels_use_the_dividers() {
        assert_eq!(channel_volume(0), (127, 0));
        assert_eq!(channel_volume(-60), (64, 0));
        assert_eq!(channel_volume(-61), (126, 1));
        assert_eq!(channel_volume(-240), (32, 2));
        assert_eq!(channel_volume(-723), (0, 3));
        assert_eq!(channel_volume(-900), (0, 3));
    }

    #[test]
    fn the_sine_is_a_full_period_over_128_steps() {
        assert_eq!(
            [sine(0), sine(16), sine(32), sine(64), sine(96)],
            [0, 90, 127, 0, -127]
        );
    }

    #[test]
    fn envelope_rates_follow_the_sdk_conversions() {
        assert_eq!(attack_rate(127), 0);
        assert_eq!(attack_rate(0x6C), 0x93);
        assert_eq!(fall_rate(127), 0xFFFF);
        assert_eq!(fall_rate(10), 21);
        assert_eq!(fall_rate(100), 0x1E00 / 26);
        assert_eq!(sustain_level(127), 0);
        assert_eq!(sustain_level(0), -92544);
    }
}

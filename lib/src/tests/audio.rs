use crate::gb::{GameBoy, GbModel};
use crate::rom::Rom;

const TONE: [u8; 22] = [
    0x3E, 0xF0, 0xE0, 0x17, 0x3E, 0x80, 0xE0, 0x16, 0x3E, 0x00, 0xE0, 0x18, 0x3E, 0x87, 0xE0, 0x19,
    0x3E, 0xFF, 0xE0, 0x25, 0x18, 0xFE,
];

fn tone_rom() -> Rom {
    let mut data = vec![0u8; 0x8000];
    data[0x0100..0x0104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]);
    data[0x0150..0x0150 + TONE.len()].copy_from_slice(&TONE);
    Rom::new(&data)
}

#[test]
fn undrained_audio_stays_bounded() {
    for sample_rate in [8_000, 44_100, 48_000, 192_000] {
        let mut gb = GameBoy::new_empty(GbModel::Dmg);
        gb.load_rom(&tone_rom()).expect("load");
        gb.apu.set_sample_rate(sample_rate);

        gb.run_cycles(5_000_000);

        let max_len = sample_rate as usize * 2;
        let len = gb.apu.audio_buffer.len();
        assert!(len > max_len / 2, "{sample_rate} Hz: {len} samples");
        assert!(len <= max_len, "{sample_rate} Hz: {len} samples");
        assert!(gb.apu.audio_buffer.iter().any(|s| s.abs() > 0.01));
    }
}

fn writes_rom(writes: &[(u8, u8)]) -> Rom {
    let mut code = vec![];
    for &(reg, value) in writes {
        code.extend_from_slice(&[0x3E, value, 0xE0, reg]);
    }
    code.extend_from_slice(&[0x18, 0xFE]);
    let mut data = vec![0u8; 0x8000];
    data[0x0100..0x0104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]);
    data[0x0150..0x0150 + code.len()].copy_from_slice(&code);
    Rom::new(&data)
}

fn four_channels_at_full_volume() -> Rom {
    let mut writes = vec![(0x26, 0x80), (0x24, 0x77), (0x25, 0xFF)];
    for i in 0..16 {
        writes.push((0x30 + i, if i < 8 { 0xFF } else { 0x00 }));
    }
    writes.extend_from_slice(&[
        (0x11, 0x80),
        (0x12, 0xF0),
        (0x13, 0x00),
        (0x16, 0x80),
        (0x17, 0xF0),
        (0x18, 0x00),
        (0x1A, 0x80),
        (0x1C, 0x20),
        (0x1D, 0x00),
        (0x21, 0xF0),
        (0x22, 0x00),
        (0x14, 0x87),
        (0x19, 0x87),
        (0x1E, 0x87),
        (0x23, 0x80),
    ]);
    writes_rom(&writes)
}

#[test]
fn four_full_channels_peak_at_half_scale() {
    let mut gb = GameBoy::new_empty(GbModel::Dmg);
    gb.load_rom(&four_channels_at_full_volume()).expect("load");
    gb.apu.set_sample_rate(48_000);

    let mut peak = 0.0f32;
    for _ in 0..60 {
        gb.run_frame();
        for s in gb.apu.audio_buffer.drain(..) {
            peak = peak.max(s.abs());
        }
    }

    assert!(peak > 0.45, "peak {peak}");
    assert!(peak < 0.55, "peak {peak}");
}

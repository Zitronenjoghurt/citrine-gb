use std::collections::VecDeque;

use super::firmware::Calibration;
use crate::nds::lcd::FRAME_CYCLES;

pub const TEMPERATURE0: u16 = 0x31C;
pub const TEMPERATURE1: u16 = TEMPERATURE0 + 143;
pub const SILENCE: u16 = 0x800;
pub const PRESSED_Z1: u16 = 0x400;
pub const PRESSED_Z2: u16 = 0xC00;

const START: u8 = 1 << 7;
const CHANNEL_SHIFT: u8 = 4;
const CHANNEL_MASK: u8 = 7;
const EIGHT_BIT: u8 = 1 << 3;
const RELEASED_X: u16 = 0x000;
const RELEASED_Y: u16 = 0xFFF;
const RELEASED_Z1: u16 = 0x000;
const RELEASED_Z2: u16 = 0xFFF;
const MIC_QUEUE: usize = 0x1000;

#[derive(Debug, Clone, Copy, Default)]
struct MicClock {
    start: u64,
    read: bool,
}

impl PartialEq for MicClock {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for MicClock {}

impl std::hash::Hash for MicClock {
    fn hash<H: std::hash::Hasher>(&self, _state: &mut H) {}
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Touch {
    calibration: Calibration,
    pen: Option<(u8, u8)>,
    mic: VecDeque<i16>,
    result: u16,
    eight_bit: bool,
    position: u8,
    #[cfg_attr(feature = "serde", serde(skip))]
    mic_clock: MicClock,
}

impl Touch {
    pub fn new(calibration: Calibration) -> Self {
        Self {
            calibration,
            ..Self::default()
        }
    }

    pub fn set_calibration(&mut self, calibration: Calibration) {
        self.calibration = calibration;
    }

    pub fn set_pen(&mut self, pen: Option<(u8, u8)>) {
        self.pen = pen;
    }

    pub fn pen_down(&self) -> bool {
        self.pen.is_some()
    }

    pub fn set_mic(&mut self, samples: &[i16], now: u64) {
        self.mic.clear();
        self.mic.extend(&samples[..samples.len().min(MIC_QUEUE)]);
        self.mic_clock = MicClock {
            start: now,
            read: false,
        };
    }

    pub fn mic_read(&self) -> bool {
        self.mic_clock.read
    }

    pub fn transfer(&mut self, byte: u8, now: u64) -> u8 {
        let output = match (self.position, self.eight_bit) {
            (1, false) => (self.result >> 5) as u8,
            (2, false) => (self.result << 3) as u8,
            (1, true) => (self.result >> 1) as u8,
            (2, true) => (self.result << 7) as u8,
            _ => 0,
        };
        if byte & START != 0 {
            self.convert(byte, now);
            self.position = 1;
        } else {
            self.position = self.position.saturating_add(1);
        }
        output
    }

    pub fn deselect(&mut self) {
        self.position = 0;
    }

    fn convert(&mut self, control: u8, now: u64) {
        self.eight_bit = control & EIGHT_BIT != 0;
        let value = match control >> CHANNEL_SHIFT & CHANNEL_MASK {
            0 => TEMPERATURE0,
            1 => self.adc().map_or(RELEASED_Y, |(_, y)| y),
            3 => self.pen.map_or(RELEASED_Z1, |_| PRESSED_Z1),
            4 => self.pen.map_or(RELEASED_Z2, |_| PRESSED_Z2),
            5 => self.adc().map_or(RELEASED_X, |(x, _)| x),
            6 => self.mic_sample(now),
            7 => TEMPERATURE1,
            _ => 0,
        };
        self.result = if self.eight_bit { value >> 4 } else { value };
    }

    fn adc(&self) -> Option<(u16, u16)> {
        self.pen.map(|(x, y)| self.calibration.to_adc(x, y))
    }

    fn mic_sample(&mut self, now: u64) -> u16 {
        self.mic_clock.read = true;
        let offset = now.saturating_sub(self.mic_clock.start);
        let index = offset * self.mic.len() as u64 / u64::from(FRAME_CYCLES);
        let sample = self.mic.get(index as usize).copied().unwrap_or(0);
        ((i32::from(sample) >> 4) + i32::from(SILENCE)) as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const X: u8 = 0xD0;
    const Y: u8 = 0x90;
    const Z1: u8 = 0xB0;
    const Z2: u8 = 0xC0;
    const AUX: u8 = 0xE0;
    const TEMP0: u8 = 0x80;
    const TEMP1: u8 = 0xF0;
    const BATTERY: u8 = 0xA0;

    fn convert(touch: &mut Touch, control: u8) -> u16 {
        convert_at(touch, control, 0)
    }

    fn convert_at(touch: &mut Touch, control: u8, now: u64) -> u16 {
        touch.transfer(control, now);
        let high = touch.transfer(0, 0);
        let low = touch.transfer(0, 0);
        touch.deselect();
        let both = u16::from(high) << 8 | u16::from(low);
        if control & EIGHT_BIT != 0 {
            both >> 7
        } else {
            both >> 3
        }
    }

    fn touch() -> Touch {
        Touch::new(Calibration::default())
    }

    #[test]
    fn a_released_pen_reads_x_0_and_y_fff() {
        let mut touch = touch();
        assert_eq!(convert(&mut touch, X), 0);
        assert_eq!(convert(&mut touch, Y), 0xFFF);
        assert_eq!(convert(&mut touch, Z1), 0);
        assert_eq!(convert(&mut touch, Z2), 0xFFF);
        assert!(!touch.pen_down());
    }

    #[test]
    fn the_pen_position_reads_through_the_calibration() {
        let mut touch = touch();
        touch.set_pen(Some((100, 50)));
        assert!(touch.pen_down());
        assert_eq!(convert(&mut touch, X), 100 << 4);
        assert_eq!(convert(&mut touch, Y), 50 << 4);
        assert_eq!(convert(&mut touch, Z1), PRESSED_Z1);
        assert_eq!(convert(&mut touch, Z2), PRESSED_Z2);
    }

    #[test]
    fn eight_bit_mode_keeps_the_top_bits() {
        let mut touch = touch();
        touch.set_pen(Some((100, 50)));
        assert_eq!(convert(&mut touch, X | EIGHT_BIT), 100);
        assert_eq!(convert(&mut touch, Y | EIGHT_BIT), 50);
    }

    #[test]
    fn twelve_bit_results_start_after_one_dummy_bit() {
        let mut touch = touch();
        touch.set_pen(Some((0xAB, 0)));
        assert_eq!(touch.transfer(X, 0), 0);
        assert_eq!(touch.transfer(0, 0), (0xAB0u16 >> 5) as u8);
        assert_eq!(touch.transfer(0, 0), (0xAB0u16 << 3) as u8);
        assert_eq!(touch.transfer(0, 0), 0);
    }

    #[test]
    fn a_new_control_byte_overlaps_the_last_result_byte() {
        let mut touch = touch();
        touch.set_pen(Some((0x40, 0x20)));
        touch.transfer(X, 0);
        let high = touch.transfer(0, 0);
        let low = touch.transfer(Y, 0);
        assert_eq!((u16::from(high) << 8 | u16::from(low)) >> 3, 0x400);
        let high = touch.transfer(0, 0);
        let low = touch.transfer(0, 0);
        assert_eq!((u16::from(high) << 8 | u16::from(low)) >> 3, 0x200);
    }

    #[test]
    fn temperature_and_battery_channels() {
        let mut touch = touch();
        assert_eq!(convert(&mut touch, TEMP0), TEMPERATURE0);
        assert_eq!(convert(&mut touch, TEMP1), TEMPERATURE1);
        let kelvin = u32::from(TEMPERATURE1 - TEMPERATURE0) * 8568 / 4096;
        assert!((295..=301).contains(&kelvin));
        assert_eq!(convert(&mut touch, BATTERY), 0);
    }

    #[test]
    fn the_microphone_is_silent_without_samples() {
        let mut touch = touch();
        assert_eq!(convert(&mut touch, AUX), SILENCE);
        assert_eq!(convert(&mut touch, AUX | EIGHT_BIT), 0x80);
    }

    #[test]
    fn a_microphone_read_picks_the_sample_for_its_time_in_the_frame() {
        let mut touch = touch();
        let start = 1_000;
        let quarter = |k: u64| start + u64::from(FRAME_CYCLES) * k / 4;
        touch.set_mic(&[i16::MAX, i16::MIN, 0x100, -0x100], start);
        assert_eq!(convert_at(&mut touch, AUX, start), 0xFFF);
        assert_eq!(convert_at(&mut touch, AUX, quarter(1) - 1), 0xFFF);
        assert_eq!(convert_at(&mut touch, AUX, quarter(1) + 1), 0x000);
        assert_eq!(convert_at(&mut touch, AUX, quarter(2) + 1), 0x810);
        assert_eq!(convert_at(&mut touch, AUX, quarter(3) + 1), 0x7F0);
        assert_eq!(convert_at(&mut touch, AUX, quarter(4) - 1), 0x7F0);
    }

    #[test]
    fn the_microphone_is_silent_once_its_frame_has_passed() {
        let mut touch = touch();
        touch.set_mic(&[i16::MAX; 8], 0);
        assert_eq!(
            convert_at(&mut touch, AUX, u64::from(FRAME_CYCLES)),
            SILENCE
        );
        touch.set_mic(&[], u64::from(FRAME_CYCLES));
        assert_eq!(
            convert_at(&mut touch, AUX, u64::from(FRAME_CYCLES)),
            SILENCE
        );
    }

    #[test]
    fn a_microphone_frame_keeps_at_most_the_queue_length() {
        let mut touch = touch();
        let samples: Vec<i16> = (0..MIC_QUEUE + 16).map(|index| index as i16).collect();
        touch.set_mic(&samples, 0);
        let last = u64::from(FRAME_CYCLES) - 1;
        assert_eq!(
            convert_at(&mut touch, AUX, last),
            SILENCE + (MIC_QUEUE as u16 - 1) / 16
        );
    }

    #[test]
    fn a_microphone_read_counts_until_the_next_frame() {
        let mut touch = touch();
        touch.set_mic(&[], 0);
        convert_at(&mut touch, X, 10);
        assert!(!touch.mic_read());
        convert_at(&mut touch, AUX, 20);
        assert!(touch.mic_read());
        touch.set_mic(&[], u64::from(FRAME_CYCLES));
        assert!(!touch.mic_read());
    }

    #[test]
    fn the_microphone_clock_is_not_part_of_the_state() {
        let mut read = touch();
        read.set_mic(&[], 500);
        convert_at(&mut read, AUX, 600);
        let mut fresh = touch();
        convert_at(&mut fresh, AUX, 0);
        assert_eq!(read, fresh);
    }

    #[test]
    fn deselect_ends_the_result() {
        let mut touch = touch();
        touch.set_pen(Some((0xFF, 0)));
        touch.transfer(X, 0);
        touch.deselect();
        assert_eq!(touch.transfer(0, 0), 0);
    }
}

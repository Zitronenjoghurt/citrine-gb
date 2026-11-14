use citrine_gb::console::Console;
use citrine_gb::gb::GameBoy;
use citrine_gb::gb::cartridge::Rtc;
use citrine_gb::machine::Machine;
use citrine_gb::rom::header::RomCartridgeType;

const DAY: i64 = 86_400;
const MBC3_DAYS: i64 = 512;
const DAY_HIGH: u8 = 0x01;

pub fn has_huc3_clock(gb: &GameBoy) -> bool {
    gb.cartridge.header.cartridge_type == Some(RomCartridgeType::HuC3)
}

pub fn local_unix(now: u64) -> Option<i64> {
    let at = jiff::Timestamp::from_second(now as i64).ok()?;
    let offset = jiff::tz::TimeZone::system().to_offset(at);
    Some(now as i64 + i64::from(offset.seconds()))
}

pub fn is_absolute(machine: &Machine) -> bool {
    !matches!(machine, Machine::Gb(_))
}

#[cfg(feature = "gba")]
pub fn set_absolute(machine: &mut Machine, local: i64) {
    match machine {
        Machine::Gba(gba) if gba.cart_devices().rtc => gba.set_clock(local),
        #[cfg(feature = "nds")]
        Machine::Nds(nds) => nds.set_clock(local),
        _ => {}
    }
}

#[cfg(not(feature = "gba"))]
pub fn set_absolute(_machine: &mut Machine, _local: i64) {}

pub fn shift(machine: &mut Machine, seconds: i64, now: u64) {
    let Some(gb) = machine.as_gb_mut() else {
        return;
    };
    if let Some(rtc) = gb.cartridge.rtc_mut() {
        shift_rtc(rtc, seconds);
    } else if has_huc3_clock(gb)
        && seconds > 0
        && let Some(save) = gb.battery(true, now)
    {
        gb.put_battery(save, now + seconds as u64);
    }
}

pub fn shift_rtc(rtc: &mut Rtc, seconds: i64) {
    if seconds >= 0 {
        rtc.advance_seconds(seconds as u64);
        return;
    }
    if rtc.real.halted() {
        return;
    }
    let real = &mut rtc.real;
    let total = i64::from(real.days()) * DAY
        + i64::from(real.hours.min(23)) * 3600
        + i64::from(real.minutes.min(59)) * 60
        + i64::from(real.seconds.min(59));
    let total = (total + seconds).max(0);
    let days = (total / DAY).min(MBC3_DAYS - 1);
    let rest = total % DAY;
    real.days_low = (days & 0xFF) as u8;
    real.days_high = (real.days_high & !DAY_HIGH) | ((days >> 8) as u8 & DAY_HIGH);
    real.hours = (rest / 3600) as u8;
    real.minutes = (rest / 60 % 60) as u8;
    real.seconds = (rest % 60) as u8;
}

pub fn hours_label(hours: f64) -> String {
    offset_label(hours as i64 * 3600)
}

pub fn offset_label(seconds: i64) -> String {
    if seconds == 0 {
        return "Device time".to_owned();
    }
    let sign = if seconds < 0 { "-" } else { "+" };
    let hours = seconds.unsigned_abs() / 3600;
    let (days, hours) = (hours / 24, hours % 24);
    match (days, hours) {
        (0, h) => format!("{sign}{h} h"),
        (d, 0) => format!("{sign}{d} d"),
        (d, h) => format!("{sign}{d} d {h} h"),
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{NOW, loaded};
    use super::super::*;
    use super::{offset_label, shift_rtc};
    use crate::emulator::Emulator;
    use citrine_gb::gb::cartridge::Rtc;

    #[test]
    fn the_clock_moves_back_and_forward_by_whole_seconds() {
        let mut rtc = Rtc::default();
        shift_rtc(&mut rtc, 2 * 86_400 + 3 * 3600 + 61);
        assert_eq!(rtc.real.days(), 2);
        assert_eq!(
            (rtc.real.hours, rtc.real.minutes, rtc.real.seconds),
            (3, 1, 1)
        );
        shift_rtc(&mut rtc, -(86_400 + 3600 + 2));
        assert_eq!(rtc.real.days(), 1);
        assert_eq!(
            (rtc.real.hours, rtc.real.minutes, rtc.real.seconds),
            (2, 0, 59)
        );
        shift_rtc(&mut rtc, -10 * 86_400);
        assert_eq!(rtc.real.days(), 0);
        assert_eq!(
            (rtc.real.hours, rtc.real.minutes, rtc.real.seconds),
            (0, 0, 0)
        );
    }

    #[test]
    fn moving_back_keeps_the_halt_and_carry_bits() {
        let mut rtc = Rtc::default();
        rtc.real.days_low = 0xFF;
        rtc.real.days_high = 0x81;
        shift_rtc(&mut rtc, -86_400);
        assert_eq!(rtc.real.days(), 510);
        assert_eq!(rtc.real.days_high & 0x80, 0x80);
        rtc.real.days_high |= 0x40;
        shift_rtc(&mut rtc, -86_400);
        assert_eq!(rtc.real.days(), 510);
    }

    #[test]
    fn huc3_clocks_only_move_forward() {
        let mut emulator = loaded("huc3-clock", 0xFE);
        let tail = |emulator: &mut Emulator| {
            emulator
                .debug_mut()
                .battery(true, NOW)
                .map(|save| save.to_file())
                .expect("save")
        };
        let before = tail(&mut emulator);
        super::shift(&mut emulator.seat.core_mut().machine, -3600, NOW);
        assert_eq!(tail(&mut emulator), before);
        super::shift(&mut emulator.seat.core_mut().machine, 3600, NOW);
        assert_ne!(tail(&mut emulator), before);
    }

    #[test]
    fn offsets_read_as_days_and_hours() {
        assert_eq!(offset_label(0), "Device time");
        assert_eq!(offset_label(3 * 3600), "+3 h");
        assert_eq!(offset_label(-2 * 86_400), "-2 d");
        assert_eq!(offset_label(86_400 + 5 * 3600), "+1 d 5 h");
        let mut settings = GimmickSettings::default();
        settings.set_clock_offset("game", i64::MAX);
        assert_eq!(settings.clock_offset("game"), settings::MAX_CLOCK_OFFSET);
        settings.set_clock_offset("game", 0);
        assert!(settings.clock_offsets.is_empty());
    }
}

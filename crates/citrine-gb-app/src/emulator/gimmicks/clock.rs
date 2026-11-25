pub fn local_unix(now: u64) -> Option<i64> {
    let at = jiff::Timestamp::from_second(now as i64).ok()?;
    let offset = jiff::tz::TimeZone::system().to_offset(at);
    Some(now as i64 + i64::from(offset.seconds()))
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
    use super::offset_label;
    use crate::emulator::Emulator;

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
        emulator.seat.core_mut().machine.shift_clock(-3600, NOW);
        assert_eq!(tail(&mut emulator), before);
        emulator.seat.core_mut().machine.shift_clock(3600, NOW);
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

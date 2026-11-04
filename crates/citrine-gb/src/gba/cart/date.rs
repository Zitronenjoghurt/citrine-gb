const DAY: u64 = 86_400;
const CENTURY_DAYS: u64 = 36_525;
pub const CENTURY: u64 = CENTURY_DAYS * DAY;
const UNIX_2000: i64 = 946_684_800;
const SATURDAY: u64 = 6;
const MONTH_DAYS: [u64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DateTime {
    pub year: u8,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl DateTime {
    pub fn from_seconds(seconds: u64) -> Self {
        let seconds = seconds % CENTURY;
        let mut days = seconds / DAY;
        let mut year = 0;
        while days >= year_days(year) {
            days -= year_days(year);
            year += 1;
        }
        let mut month = 0;
        while days >= month_days(year, month) {
            days -= month_days(year, month);
            month += 1;
        }
        let time = seconds % DAY;
        Self {
            year: year as u8,
            month: month as u8 + 1,
            day: days as u8 + 1,
            hour: (time / 3600) as u8,
            minute: (time / 60 % 60) as u8,
            second: (time % 60) as u8,
        }
    }

    pub fn to_seconds(self) -> u64 {
        let year = u64::from(self.year.min(99));
        let month = u64::from(self.month.clamp(1, 12)) - 1;
        let days = (0..year).map(year_days).sum::<u64>()
            + (0..month).map(|m| month_days(year, m)).sum::<u64>()
            + u64::from(self.day.max(1))
            - 1;
        days * DAY
            + u64::from(self.hour.min(23)) * 3600
            + u64::from(self.minute.min(59)) * 60
            + u64::from(self.second.min(59))
    }
}

pub fn weekday(seconds: u64) -> u8 {
    ((seconds % CENTURY / DAY + SATURDAY) % 7) as u8
}

pub fn from_unix(unix: i64) -> u64 {
    (unix - UNIX_2000).rem_euclid(CENTURY as i64) as u64
}

fn year_days(year: u64) -> u64 {
    if year.is_multiple_of(4) { 366 } else { 365 }
}

fn month_days(year: u64, month: u64) -> u64 {
    if month == 1 && year.is_multiple_of(4) {
        29
    } else {
        MONTH_DAYS[month as usize]
    }
}

pub fn to_bcd(value: u8) -> u8 {
    ((value / 10) << 4) | (value % 10)
}

pub fn from_bcd(value: u8) -> u8 {
    (value >> 4) * 10 + (value & 0x0F)
}

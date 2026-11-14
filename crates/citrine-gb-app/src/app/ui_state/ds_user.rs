use citrine_gb::rom::nds::NdsLanguage;
use std::fmt::Display;
use strum_macros::EnumIter;

pub const NICKNAME_CHARS: usize = 10;
pub const FALLBACK_NICKNAME: &str = "Citrine";

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum DsLanguage {
    Japanese,
    #[default]
    English,
    French,
    German,
    Italian,
    Spanish,
}

impl DsLanguage {
    pub fn of_app() -> Self {
        sys_locale::get_locales()
            .find_map(|tag| Self::from_tag(&tag))
            .unwrap_or_default()
    }

    fn from_tag(tag: &str) -> Option<Self> {
        Some(match NdsLanguage::from_tag(tag)? {
            NdsLanguage::Japanese => Self::Japanese,
            NdsLanguage::English => Self::English,
            NdsLanguage::French => Self::French,
            NdsLanguage::German => Self::German,
            NdsLanguage::Italian => Self::Italian,
            NdsLanguage::Spanish => Self::Spanish,
            NdsLanguage::Chinese | NdsLanguage::Korean => return None,
        })
    }
}

impl Display for DsLanguage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Japanese => "日本語",
            Self::English => "English",
            Self::French => "Français",
            Self::German => "Deutsch",
            Self::Italian => "Italiano",
            Self::Spanish => "Español",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DsUser {
    pub nickname: Option<String>,
    pub birthday: (u8, u8),
    pub language: Option<DsLanguage>,
    pub favorite_color: u8,
    pub keep_firmware_settings: bool,
    pub clock: Option<i64>,
}

impl Default for DsUser {
    fn default() -> Self {
        Self {
            nickname: None,
            birthday: (1, 1),
            language: None,
            favorite_color: 0,
            keep_firmware_settings: false,
            clock: None,
        }
    }
}

impl DsUser {
    pub fn nickname(&self, account: Option<&str>) -> String {
        let name = self
            .nickname
            .as_deref()
            .or(account)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(FALLBACK_NICKNAME);
        name.chars().take(NICKNAME_CHARS).collect()
    }

    pub fn set_nickname(&mut self, text: &str) {
        let text: String = text.chars().take(NICKNAME_CHARS).collect();
        self.nickname = (!text.trim().is_empty()).then_some(text);
    }

    pub fn language(&self) -> DsLanguage {
        self.language.unwrap_or_else(DsLanguage::of_app)
    }

    pub fn set_birthday(&mut self, month: u8, day: u8) {
        let month = month.clamp(1, 12);
        self.birthday = (month, day.clamp(1, days_in(month)));
    }

    #[cfg(feature = "nds")]
    pub fn user_settings(
        &self,
        account: Option<&str>,
    ) -> citrine_gb::nds::spi::firmware::UserSettings {
        use citrine_gb::nds::spi::firmware::{Language, UserSettings};
        UserSettings {
            nickname: self.nickname(account),
            favorite_color: self.favorite_color & 0x0F,
            birthday: self.birthday,
            language: match self.language() {
                DsLanguage::Japanese => Language::Japanese,
                DsLanguage::English => Language::English,
                DsLanguage::French => Language::French,
                DsLanguage::German => Language::German,
                DsLanguage::Italian => Language::Italian,
                DsLanguage::Spanish => Language::Spanish,
            },
            ..UserSettings::default()
        }
    }
}

#[cfg(feature = "nds")]
pub const COLORS: [(u32, &str); 16] = [
    (0x63849C, "Gray"),
    (0xBD4A00, "Brown"),
    (0xFF0019, "Red"),
    (0xFF8CFF, "Pink"),
    (0xFF9400, "Orange"),
    (0xF7E600, "Yellow"),
    (0xADFF00, "Lime"),
    (0x00FF00, "Green"),
    (0x00A53A, "Dark green"),
    (0x4ADE8C, "Sea green"),
    (0x31BDF7, "Turquoise"),
    (0x005AF7, "Blue"),
    (0x000094, "Dark blue"),
    (0x8C00D6, "Purple"),
    (0xD600EF, "Violet"),
    (0xFF0094, "Magenta"),
];

#[cfg(any(feature = "nds", test))]
pub const SYSTEM_TIME: &str = "System time";
#[cfg(feature = "nds")]
pub const CUSTOM_TIME: &str = "Custom";
#[cfg(any(feature = "nds", test))]
const FIRST_YEAR: i16 = 2000;
#[cfg(any(feature = "nds", test))]
const LAST_YEAR: i16 = 2099;

#[cfg(any(feature = "nds", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClockPart {
    Date,
    Time,
}

#[cfg(feature = "nds")]
impl ClockPart {
    pub fn label(self) -> &'static str {
        match self {
            Self::Date => "Date",
            Self::Time => "Time",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::Date => "YYYY-MM-DD",
            Self::Time => "HH:MM",
        }
    }
}

#[cfg(any(feature = "nds", test))]
fn clock_at(offset: i64, now: u64) -> Option<jiff::civil::DateTime> {
    let local = crate::emulator::gimmicks::clock::local_unix(now)?;
    let at = jiff::Timestamp::from_second(local + offset).ok()?;
    Some(at.to_zoned(jiff::tz::TimeZone::UTC).datetime())
}

#[cfg(any(feature = "nds", test))]
pub fn clock_text(offset: i64, now: u64, part: ClockPart) -> String {
    let Some(at) = clock_at(offset, now) else {
        return String::new();
    };
    match part {
        ClockPart::Date => at.strftime("%Y-%m-%d").to_string(),
        ClockPart::Time => at.strftime("%H:%M").to_string(),
    }
}

#[cfg(any(feature = "nds", test))]
pub fn clock_label(clock: Option<i64>, now: u64) -> String {
    match clock {
        None => SYSTEM_TIME.to_owned(),
        Some(offset) => format!(
            "{} {}",
            clock_text(offset, now, ClockPart::Date),
            clock_text(offset, now, ClockPart::Time)
        ),
    }
}

#[cfg(any(feature = "nds", test))]
pub fn parse_clock(offset: i64, now: u64, part: ClockPart, text: &str) -> Option<i64> {
    let at = clock_at(offset, now)?;
    let text = text.trim();
    let set = match part {
        ClockPart::Date => {
            let date: jiff::civil::Date = text.parse().ok()?;
            date.to_datetime(at.time())
        }
        ClockPart::Time => {
            let time = jiff::civil::Time::strptime("%H:%M", text).ok()?;
            at.date().to_datetime(time)
        }
    };
    if !(FIRST_YEAR..=LAST_YEAR).contains(&set.year()) {
        return None;
    }
    let seconds = set
        .to_zoned(jiff::tz::TimeZone::UTC)
        .ok()?
        .timestamp()
        .as_second();
    let local = crate::emulator::gimmicks::clock::local_unix(now)?;
    Some(seconds - local)
}

pub fn days_in(month: u8) -> u8 {
    match month {
        2 => 29,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nickname_follows_the_account_cut_to_ten() {
        let user = DsUser::default();
        assert_eq!(user.nickname(Some("Clementine Zitrone")), "Clementine");
        assert_eq!(user.nickname(Some("Clara")), "Clara");
        assert_eq!(user.nickname(None), FALLBACK_NICKNAME);
    }

    #[test]
    fn a_typed_nickname_wins_until_it_is_cleared() {
        let mut user = DsUser::default();
        user.set_nickname("Zitrone und mehr");
        assert_eq!(user.nickname.as_deref(), Some("Zitrone un"));
        assert_eq!(user.nickname(Some("Clara")), "Zitrone un");
        user.set_nickname("  ");
        assert_eq!(user.nickname, None);
        assert_eq!(user.nickname(Some("Clara")), "Clara");
    }

    #[test]
    fn a_birthday_stays_a_real_date() {
        let mut user = DsUser::default();
        user.set_birthday(2, 31);
        assert_eq!(user.birthday, (2, 29));
        user.set_birthday(13, 0);
        assert_eq!(user.birthday, (12, 1));
    }

    #[test]
    fn the_language_follows_the_app_until_picked() {
        assert_eq!(DsLanguage::from_tag("de-DE"), Some(DsLanguage::German));
        assert_eq!(DsLanguage::from_tag("ko-KR"), None);
        let user = DsUser {
            language: Some(DsLanguage::Italian),
            ..DsUser::default()
        };
        assert_eq!(user.language(), DsLanguage::Italian);
    }

    #[test]
    fn old_settings_load_with_the_defaults() {
        let user: DsUser = serde_json::from_str("{}").unwrap();
        assert_eq!(user, DsUser::default());
    }

    #[test]
    fn an_own_time_reads_back_and_runs_on() {
        let now = 1_762_358_400;
        let date = parse_clock(0, now, ClockPart::Date, "2009-02-13").unwrap();
        let offset = parse_clock(date, now, ClockPart::Time, "23:31").unwrap();
        assert_eq!(clock_label(Some(offset), now), "2009-02-13 23:31");
        assert_eq!(clock_label(Some(offset), now + 90 * 60), "2009-02-14 01:01");
        assert_eq!(clock_label(None, now), SYSTEM_TIME);
    }

    #[test]
    fn a_time_the_ds_cannot_hold_is_refused() {
        let now = 1_762_358_400;
        assert_eq!(parse_clock(0, now, ClockPart::Date, "1999-12-31"), None);
        assert_eq!(parse_clock(0, now, ClockPart::Date, "2100-01-01"), None);
        assert_eq!(parse_clock(0, now, ClockPart::Date, "13.02.2009"), None);
        assert_eq!(parse_clock(0, now, ClockPart::Time, "24:00"), None);
        assert!(parse_clock(0, now, ClockPart::Time, " 7:05 ").is_some());
    }
}

use jiff::civil::Date;
use jiff::tz::TimeZone;

pub(super) struct Clock {
    pub(super) zone: TimeZone,
    pub(super) today: Date,
}

impl Clock {
    pub(super) fn new() -> Self {
        let zone = TimeZone::system();
        let today = jiff::Timestamp::now().to_zoned(zone.clone()).date();
        Self { zone, today }
    }

    pub(super) fn zoned(&self, at: u64) -> Option<jiff::Zoned> {
        jiff::Timestamp::from_second(at as i64)
            .ok()
            .map(|t| t.to_zoned(self.zone.clone()))
    }

    pub(super) fn day(&self, at: u64) -> Option<Date> {
        self.zoned(at).map(|z| z.date())
    }

    pub(super) fn time(&self, at: u64) -> String {
        self.zoned(at)
            .map(|z| z.strftime("%H:%M").to_string())
            .unwrap_or_default()
    }

    pub(super) fn day_label(&self, day: Date) -> String {
        if day == self.today {
            "Today".to_owned()
        } else if self.today.yesterday().is_ok_and(|y| y == day) {
            "Yesterday".to_owned()
        } else {
            day.strftime("%-d %b %Y").to_string()
        }
    }
}

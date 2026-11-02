use crate::game_info::Game;
use egui::Color32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Saved,
    Behind,
    File,
}

impl Status {
    pub(super) fn color(self, dark: bool) -> Color32 {
        match (self, dark) {
            (Status::Saved, true) => Color32::from_rgb(0x4c, 0xaf, 0x50),
            (Status::Saved, false) => Color32::from_rgb(0x2e, 0x9a, 0x3a),
            (Status::Behind, true) => Color32::from_rgb(0xe3, 0xa5, 0x1a),
            (Status::Behind, false) => Color32::from_rgb(0xc9, 0x8a, 0x00),
            (Status::File, _) => Color32::from_gray(0x8c),
        }
    }
}

pub struct Clock {
    zone: jiff::tz::TimeZone,
    today: jiff::civil::Date,
}

impl Clock {
    pub fn new() -> Self {
        Self::in_zone(jiff::tz::TimeZone::system(), jiff::Timestamp::now())
    }

    pub fn in_zone(zone: jiff::tz::TimeZone, now: jiff::Timestamp) -> Self {
        let today = now.to_zoned(zone.clone()).date();
        Self { zone, today }
    }

    pub(super) fn when(&self, at: u64) -> String {
        let Ok(stamp) = jiff::Timestamp::from_second(at as i64) else {
            return String::new();
        };
        let zoned = stamp.to_zoned(self.zone.clone());
        if zoned.date() == self.today {
            zoned.strftime("%H:%M").to_string()
        } else {
            zoned.strftime("%-d %b, %H:%M").to_string()
        }
    }
}

pub fn status_line(game: &Game, clock: &Clock) -> (Status, String) {
    let when = game.saved_at.map(|at| clock.when(at));
    match (game.live.is_some(), game.behind, game.saved.is_some(), when) {
        (true, false, true, Some(when)) => {
            (Status::Saved, format!("Playing, saved in game at {when}"))
        }
        (true, false, true, None) => (Status::Saved, "Playing, saved in game".to_owned()),
        (true, _, true, Some(when)) => (
            Status::Behind,
            format!("Playing, not saved in game since {when}"),
        ),
        (true, _, _, _) => (Status::Behind, "Playing, not saved in game yet".to_owned()),
        (false, _, true, Some(when)) => (Status::File, format!("Save file from {when}")),
        (false, _, true, None) => (Status::File, "Save file".to_owned()),
        (false, _, false, _) => (Status::File, "No saved game yet".to_owned()),
    }
}

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

#[cfg(test)]
mod tests {
    use super::super::tests::{NOON, clock, crystal};
    use super::{Status, status_line};
    use citrine_gb_gamedata::Ram;
    use citrine_gb_gamedata::fixtures::{crystal_live_wram, crystal_save, set_crystal_live_money};

    #[test]
    fn the_line_says_whether_the_live_game_is_saved() {
        let save = crystal_save();
        let mut wram = crystal_live_wram(&save);
        let mut info = crystal(Some(NOON as u64 - 3600));
        info.read(&save, Ram::Gb(&wram), NOON as u64);
        assert_eq!(
            status_line(info.game().unwrap(), &clock()),
            (Status::Saved, "Playing, saved in game at 11:00".to_owned())
        );

        set_crystal_live_money(&mut wram, 4815);
        info.read(&save, Ram::Gb(&wram), NOON as u64);
        assert_eq!(
            status_line(info.game().unwrap(), &clock()),
            (
                Status::Behind,
                "Playing, not saved in game since 11:00".to_owned()
            )
        );
    }

    #[test]
    fn the_line_shows_the_save_file_and_its_day_when_the_game_is_not_running() {
        let mut info = crystal(Some(NOON as u64 - 3 * 86_400));
        info.read(&crystal_save(), Ram::Gb(&[0u8; 0x2000]), NOON as u64);
        assert_eq!(
            status_line(info.game().unwrap(), &clock()),
            (Status::File, "Save file from 28 Oct, 12:00".to_owned())
        );

        let mut empty = crystal(None);
        empty.read(&vec![0u8; 0x8000], Ram::Gb(&[0u8; 0x2000]), NOON as u64);
        assert_eq!(
            status_line(empty.game().unwrap(), &clock()),
            (Status::File, "No saved game yet".to_owned())
        );
    }
}

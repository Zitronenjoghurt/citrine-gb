use super::CablePartner;
use super::record::{Book, Progress, Record};
use std::time::Duration;

pub const SESSION_GAP: u64 = 30 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Session {
    rom: String,
    played_ms: u64,
    last_play: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CableRun {
    rom: String,
    partner: String,
    last: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Activity {
    pub linked: bool,
    pub together: bool,
    pub fast_forward: bool,
}

#[derive(Debug, Default)]
pub struct Recorder {
    book: Book,
    session: Option<Session>,
    launched: Option<(String, u64)>,
    carry: Duration,
    cable_run: Option<CableRun>,
    cable_carry: Duration,
    dirty: bool,
}

impl Recorder {
    pub fn new(book: Book) -> Self {
        Self {
            book,
            ..Default::default()
        }
    }

    pub fn book(&self) -> &Book {
        &self.book
    }

    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    pub fn session_ms(&self, rom: &str) -> Option<u64> {
        self.session
            .as_ref()
            .filter(|s| s.rom == rom)
            .map(|s| s.played_ms)
    }

    fn own(&mut self) -> &mut Record {
        self.dirty = true;
        self.book.own()
    }

    fn name(&mut self, rom: &str, name: &str) {
        if !name.is_empty() {
            let game = self.own().game(rom);
            if game.name != name {
                game.name = name.to_string();
            }
        }
    }

    pub fn started_at(&self, rom: &str) -> Option<u64> {
        self.launched
            .as_ref()
            .filter(|(r, _)| r == rom)
            .map(|(_, at)| *at)
    }

    pub fn launch(&mut self, rom: &str, name: &str, now: u64, day: &str) {
        self.session = None;
        self.launched = Some((rom.to_string(), now));
        self.carry = Duration::ZERO;
        self.name(rom, name);
        self.own().game_day(rom, day).launches += 1;
    }

    pub fn play(
        &mut self,
        rom: &str,
        name: &str,
        played: Duration,
        activity: Activity,
        now: u64,
        day: &str,
    ) {
        let total = self.carry + played;
        let ms = total.as_millis() as u64;
        self.carry = total.saturating_sub(Duration::from_millis(ms));
        if ms == 0 {
            return;
        }
        let fresh = match &self.session {
            Some(session) => {
                session.rom != rom || now.saturating_sub(session.last_play) > SESSION_GAP
            }
            None => true,
        };
        if fresh {
            self.session = Some(Session {
                rom: rom.to_string(),
                played_ms: 0,
                last_play: now,
            });
        }
        let session_ms = {
            let session = self.session.as_mut().expect("a session was just started");
            session.played_ms += ms;
            session.last_play = now;
            session.played_ms
        };
        self.name(rom, name);
        let record = self.own();
        let game = record.game(rom);
        if game.first_played == 0 {
            game.first_played = now;
        }
        game.last_played = game.last_played.max(now);
        game.longest_session_ms = game.longest_session_ms.max(session_ms);
        let counts = record.game_day(rom, day);
        counts.play_ms += ms;
        if fresh {
            counts.sessions += 1;
        }
        if activity.linked {
            counts.link_ms += ms;
        }
        if activity.together {
            counts.together_ms += ms;
        }
        if activity.fast_forward {
            counts.fast_forward_ms += ms;
        }
    }

    pub fn cable(
        &mut self,
        rom: &str,
        partner: &CablePartner,
        name: &str,
        played: Duration,
        now: u64,
        day: &str,
    ) {
        let total = self.cable_carry + played;
        let ms = total.as_millis() as u64;
        self.cable_carry = total.saturating_sub(Duration::from_millis(ms));
        if ms == 0 {
            return;
        }
        let key = partner.key();
        let fresh = self.cable_run.as_ref().is_none_or(|run| {
            run.rom != rom || run.partner != key || now.saturating_sub(run.last) > SESSION_GAP
        });
        self.cable_run = Some(CableRun {
            rom: rom.to_string(),
            partner: key.clone(),
            last: now,
        });
        let record = self.own();
        if !name.is_empty() {
            let partners = &mut record.game(rom).partners;
            if partners.get(&key).is_none_or(|known| known != name) {
                partners.insert(key.clone(), name.to_string());
            }
        }
        let counts = record.game_day(rom, day).cable.entry(key).or_default();
        counts.ms += ms;
        if fresh {
            counts.sessions += 1;
        }
    }

    pub fn snapshot(&mut self, rom: &str, day: &str) {
        self.own().game_day(rom, day).snapshots += 1;
    }

    pub fn load(&mut self, rom: &str, day: &str) {
        self.own().game_day(rom, day).loads += 1;
    }

    pub fn rewind(&mut self, rom: &str, day: &str) {
        self.own().game_day(rom, day).rewinds += 1;
    }

    pub fn chat_message(&mut self, day: &str) {
        self.own().app_day(day).chat_messages += 1;
    }

    pub fn progress(&mut self, rom: &str, day: &str, progress: Progress) {
        let game = self.book.own().game(rom);
        let entry = game.progress.entry(day.to_string()).or_default();
        if *entry != progress {
            *entry = progress;
            self.dirty = true;
        }
    }

    pub fn merge(&mut self, other: &Book) {
        let before = self.book.clone();
        self.book.merge(other);
        if self.book != before {
            self.dirty = true;
        }
    }

    pub fn merge_device(&mut self, device: &str, record: &Record) {
        if device == self.book.device {
            return;
        }
        let before = self.book.devices.get(device).cloned();
        self.book.merge_device(device, record);
        if self.book.devices.get(device) != before.as_ref() {
            self.dirty = true;
        }
    }

    pub fn clear(&mut self) {
        self.book = Book::new();
        self.session = None;
        self.carry = Duration::ZERO;
        self.cable_run = None;
        self.cable_carry = Duration::ZERO;
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROM: &str = "aa";
    const DAY: &str = "2026-10-31";

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn play_time_adds_up_by_day_and_session() {
        let mut stats = Recorder::new(Book::new());
        stats.launch(ROM, "Pocket Quest", 0, DAY);
        stats.play(
            ROM,
            "Pocket Quest",
            secs(60),
            Activity::default(),
            1000,
            DAY,
        );
        stats.play(
            ROM,
            "Pocket Quest",
            secs(30),
            Activity::default(),
            1090,
            DAY,
        );
        stats.play(
            ROM,
            "Pocket Quest",
            secs(10),
            Activity::default(),
            1090 + SESSION_GAP + 1,
            "2026-11-01",
        );
        let game = &stats.book().own_record().unwrap().games[ROM];
        assert_eq!(game.name, "Pocket Quest");
        assert_eq!(game.days[DAY].play_ms, 90_000);
        assert_eq!(game.days[DAY].sessions, 1);
        assert_eq!(game.days[DAY].launches, 1);
        assert_eq!(game.days["2026-11-01"].sessions, 1);
        assert_eq!(game.days["2026-11-01"].play_ms, 10_000);
        assert_eq!(game.longest_session_ms, 90_000);
        assert_eq!(game.first_played, 1000);
        assert_eq!(game.last_played, 1090 + SESSION_GAP + 1);
    }

    #[test]
    fn short_frames_carry_their_remainder() {
        let mut stats = Recorder::new(Book::new());
        for n in 0..1000 {
            stats.play(
                ROM,
                "",
                Duration::from_micros(16_667),
                Activity::default(),
                n,
                DAY,
            );
        }
        let game = &stats.book().own_record().unwrap().games[ROM];
        assert_eq!(game.days[DAY].play_ms, 16_667);
    }

    #[test]
    fn a_new_launch_starts_a_new_session() {
        let mut stats = Recorder::new(Book::new());
        stats.launch(ROM, "", 0, DAY);
        stats.play(ROM, "", secs(5), Activity::default(), 10, DAY);
        stats.launch("bb", "", 0, DAY);
        stats.play("bb", "", secs(5), Activity::default(), 20, DAY);
        stats.launch(ROM, "", 0, DAY);
        stats.play(ROM, "", secs(7), Activity::default(), 30, DAY);
        let game = &stats.book().own_record().unwrap().games[ROM];
        assert_eq!(game.days[DAY].sessions, 2);
        assert_eq!(game.days[DAY].launches, 2);
        assert_eq!(game.longest_session_ms, 7000);
    }

    #[test]
    fn link_together_and_counters_are_kept_per_game() {
        let mut stats = Recorder::new(Book::new());
        let linked = Activity {
            linked: true,
            ..Default::default()
        };
        let together = Activity {
            together: true,
            ..Default::default()
        };
        stats.play(ROM, "", secs(3), linked, 1, DAY);
        stats.play(ROM, "", secs(4), together, 2, DAY);
        stats.snapshot(ROM, DAY);
        stats.load(ROM, DAY);
        stats.load(ROM, DAY);
        stats.rewind(ROM, DAY);
        stats.chat_message(DAY);
        let record = stats.book().own_record().unwrap();
        let day = &record.games[ROM].days[DAY];
        assert_eq!((day.link_ms, day.together_ms), (3000, 4000));
        assert_eq!((day.snapshots, day.loads, day.rewinds), (1, 2, 1));
        assert_eq!(record.days[DAY].chat_messages, 1);
    }

    #[test]
    fn cable_time_and_sessions_are_kept_per_partner() {
        let mut stats = Recorder::new(Book::new());
        let mia = CablePartner::Friend("7".into());
        let red = CablePartner::Game("bb".into());
        stats.cable(ROM, &mia, "Mia", secs(60), 100, DAY);
        stats.cable(ROM, &mia, "Mia", secs(30), 160, DAY);
        stats.cable(ROM, &red, "Pokémon Red", secs(20), 200, DAY);
        stats.cable(ROM, &mia, "Mia", secs(10), 220, DAY);
        stats.cable(ROM, &mia, "Mia", secs(5), 220 + SESSION_GAP + 1, DAY);
        let game = &stats.book().own_record().unwrap().games[ROM];
        let mia_day = game.days[DAY].cable[&mia.key()];
        let red_day = game.days[DAY].cable[&red.key()];
        assert_eq!((mia_day.ms, mia_day.sessions), (105_000, 3));
        assert_eq!((red_day.ms, red_day.sessions), (20_000, 1));
        assert_eq!(game.partners[&mia.key()], "Mia");
        assert_eq!(game.partners[&red.key()], "Pokémon Red");
    }

    #[test]
    fn the_overview_adds_cable_time_across_games_and_devices() {
        use crate::stats::{CableSum, cable_sum};
        let mut stats = Recorder::new(Book::new());
        let mia = CablePartner::Friend("7".into());
        let celebi = CablePartner::Event("celebi".into());
        stats.cable(ROM, &mia, "Mia", secs(60), 100, DAY);
        stats.cable("bb", &mia, "Mia", secs(30), 200, DAY);
        stats.cable("bb", &celebi, "Celebi", secs(10), 300, DAY);
        let mut other = Record::default();
        other.game_day(ROM, DAY).cable.insert(
            mia.key(),
            super::super::record::CableDay {
                ms: 15_000,
                sessions: 1,
            },
        );
        stats.merge_device("other", &other);
        let overview = stats
            .book()
            .overview(jiff::civil::Date::new(2026, 10, 31).unwrap());
        assert_eq!(overview.cable[0].partner, mia);
        assert_eq!(overview.cable[0].name, "Mia");
        assert_eq!(
            cable_sum(&overview.cable, true),
            CableSum {
                ms: 105_000,
                sessions: 3
            }
        );
        assert_eq!(
            cable_sum(&overview.cable, false),
            CableSum {
                ms: 10_000,
                sessions: 1
            }
        );
    }

    #[test]
    fn copies_from_other_devices_never_touch_this_one() {
        let mut stats = Recorder::new(Book::new());
        stats.play(ROM, "", secs(10), Activity::default(), 1, DAY);
        stats.take_dirty();
        let own = stats.book().device.clone();
        let mut fake = Record::default();
        fake.game_day(ROM, DAY).play_ms = 999_000;
        stats.merge_device(&own, &fake);
        assert!(!stats.take_dirty());
        stats.merge_device("other", &fake);
        assert!(stats.take_dirty());
        stats.merge_device("other", &fake);
        assert!(!stats.take_dirty());
        assert_eq!(
            stats.book().own_record().unwrap().games[ROM].days[DAY].play_ms,
            10_000
        );
    }

    #[test]
    fn the_same_progress_twice_needs_no_sync() {
        let mut stats = Recorder::new(Book::new());
        let progress = Progress {
            in_game_secs: 3600,
            dex_owned: 12,
            dex_seen: 30,
            badges: 2,
        };
        stats.progress(ROM, DAY, progress);
        assert!(stats.take_dirty());
        stats.progress(ROM, DAY, progress);
        assert!(!stats.take_dirty());
        assert_eq!(
            stats.book().own_record().unwrap().games[ROM].progress[DAY],
            progress
        );
    }
}

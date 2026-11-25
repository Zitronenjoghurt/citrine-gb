mod record;
mod recorder;
mod store;

pub use record::{Book, GameRecord, Progress};
pub use recorder::Activity;
pub use store::{Stats, StatsStore};

use jiff::civil::Date;
use std::collections::BTreeMap;

thread_local! {
    static ZONE: jiff::tz::TimeZone = jiff::tz::TimeZone::system();
}

pub fn local_date(unix: u64) -> Date {
    let at = jiff::Timestamp::from_second(unix as i64).unwrap_or(jiff::Timestamp::UNIX_EPOCH);
    ZONE.with(|zone| at.to_zoned(zone.clone()).date())
}

pub fn today() -> Date {
    local_date(crate::storage::now_unix())
}

pub fn day_key(date: Date) -> String {
    date.to_string()
}

pub fn parse_day(key: &str) -> Option<Date> {
    key.parse().ok()
}

pub fn week_start(date: Date) -> Date {
    let back = i64::from(date.weekday().to_monday_zero_offset());
    date.checked_sub(jiff::Span::new().days(back))
        .unwrap_or(date)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CablePartner {
    Game(String),
    Event(String),
    Friend(String),
}

impl CablePartner {
    pub fn key(&self) -> String {
        match self {
            Self::Game(rom) => format!("game:{rom}"),
            Self::Event(event) => format!("event:{event}"),
            Self::Friend(id) => format!("friend:{id}"),
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        let (kind, id) = key.split_once(':')?;
        let id = id.to_string();
        match kind {
            "game" => Some(Self::Game(id)),
            "event" => Some(Self::Event(id)),
            "friend" => Some(Self::Friend(id)),
            _ => None,
        }
    }

    pub fn online(&self) -> bool {
        matches!(self, Self::Friend(_))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CableTotals {
    pub partner: CablePartner,
    pub name: String,
    pub ms: u64,
    pub sessions: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CableSum {
    pub ms: u64,
    pub sessions: u64,
}

pub fn cable_sum(cable: &[CableTotals], online: bool) -> CableSum {
    cable
        .iter()
        .filter(|totals| totals.partner.online() == online)
        .fold(CableSum::default(), |sum, totals| CableSum {
            ms: sum.ms + totals.ms,
            sessions: sum.sessions + totals.sessions,
        })
}

fn add_cable(into: &mut Vec<CableTotals>, from: &CableTotals) {
    match into
        .iter_mut()
        .find(|totals| totals.partner == from.partner)
    {
        Some(totals) => {
            totals.ms += from.ms;
            totals.sessions += from.sessions;
            if totals.name.is_empty() {
                totals.name = from.name.clone();
            }
        }
        None => into.push(from.clone()),
    }
}

fn sort_cable(cable: &mut [CableTotals]) {
    cable.sort_by(|a, b| b.ms.cmp(&a.ms).then_with(|| a.name.cmp(&b.name)));
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GameTotals {
    pub rom: String,
    pub name: String,
    pub play_ms: u64,
    pub today_ms: u64,
    pub week_ms: u64,
    pub sessions: u64,
    pub launches: u64,
    pub longest_ms: u64,
    pub first_played: u64,
    pub last_played: u64,
    pub snapshots: u64,
    pub loads: u64,
    pub link_ms: u64,
    pub together_ms: u64,
    pub cable: Vec<CableTotals>,
    pub days: BTreeMap<String, u64>,
    pub progress: BTreeMap<String, Progress>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Overview {
    pub play_ms: u64,
    pub today_ms: u64,
    pub week_ms: u64,
    pub games_played: usize,
    pub sessions: u64,
    pub snapshots: u64,
    pub loads: u64,
    pub fast_forward_ms: u64,
    pub rewinds: u64,
    pub link_ms: u64,
    pub together_ms: u64,
    pub chat_messages: u64,
    pub cable: Vec<CableTotals>,
    pub days: BTreeMap<String, u64>,
    pub games: Vec<GameTotals>,
}

impl Overview {
    pub fn most_played(&self) -> Option<&GameTotals> {
        self.games.first().filter(|game| game.play_ms > 0)
    }

    pub fn game(&self, rom: &str) -> Option<&GameTotals> {
        self.games.iter().find(|game| game.rom == rom)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PeriodGame {
    pub rom: String,
    pub name: String,
    pub play_ms: u64,
    pub sessions: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Period {
    pub play_ms: u64,
    pub sessions: u64,
    pub snapshots: u64,
    pub loads: u64,
    pub link_ms: u64,
    pub together_ms: u64,
    pub chat_messages: u64,
    pub cable: Vec<CableTotals>,
    pub games: Vec<PeriodGame>,
}

impl Book {
    pub fn period(&self, first: Date, last: Date, rom: Option<&str>) -> Period {
        let (first, last) = (day_key(first), day_key(last));
        let within = |key: &String| *key >= first && *key <= last;
        let mut period = Period::default();
        let mut games: BTreeMap<&str, (PeriodGame, u64)> = BTreeMap::new();
        for record in self.devices.values() {
            if rom.is_none() {
                for (_, app) in record.days.iter().filter(|(key, _)| within(key)) {
                    period.chat_messages += u64::from(app.chat_messages);
                }
            }
            for (key, game) in &record.games {
                if rom.is_some_and(|rom| rom != key) {
                    continue;
                }
                let (totals, named_at) = games.entry(key).or_insert_with(|| {
                    let game = PeriodGame {
                        rom: key.clone(),
                        ..Default::default()
                    };
                    (game, 0)
                });
                if !game.name.is_empty() && *named_at <= game.last_played {
                    totals.name = game.name.clone();
                    *named_at = game.last_played;
                }
                for (_, counts) in game.days.iter().filter(|(day, _)| within(day)) {
                    totals.play_ms += counts.play_ms;
                    totals.sessions += u64::from(counts.sessions);
                    period.snapshots += u64::from(counts.snapshots);
                    period.loads += u64::from(counts.loads);
                    period.link_ms += counts.link_ms;
                    period.together_ms += counts.together_ms;
                    for (partner_key, cable) in &counts.cable {
                        let Some(partner) = CablePartner::from_key(partner_key) else {
                            continue;
                        };
                        let name = game.partners.get(partner_key).cloned().unwrap_or_default();
                        add_cable(
                            &mut period.cable,
                            &CableTotals {
                                partner,
                                name,
                                ms: cable.ms,
                                sessions: u64::from(cable.sessions),
                            },
                        );
                    }
                }
            }
        }
        sort_cable(&mut period.cable);
        period.games = games
            .into_values()
            .map(|(game, _)| game)
            .filter(|game| game.play_ms > 0)
            .collect();
        period
            .games
            .sort_by(|a, b| b.play_ms.cmp(&a.play_ms).then_with(|| a.name.cmp(&b.name)));
        period.play_ms = period.games.iter().map(|game| game.play_ms).sum();
        period.sessions = period.games.iter().map(|game| game.sessions).sum();
        period
    }
}

impl Book {
    pub fn overview(&self, today: Date) -> Overview {
        let today_key = day_key(today);
        let week_key = day_key(week_start(today));
        let mut overview = Overview::default();
        let mut games: BTreeMap<String, GameTotals> = BTreeMap::new();
        let mut named_at: BTreeMap<String, u64> = BTreeMap::new();
        for record in self.devices.values() {
            for app in record.days.values() {
                overview.chat_messages += u64::from(app.chat_messages);
            }
            for (rom, game) in &record.games {
                let totals = games.entry(rom.clone()).or_insert_with(|| GameTotals {
                    rom: rom.clone(),
                    ..Default::default()
                });
                if !game.name.is_empty()
                    && named_at.get(rom).is_none_or(|at| *at <= game.last_played)
                {
                    totals.name = game.name.clone();
                    named_at.insert(rom.clone(), game.last_played);
                }
                totals.longest_ms = totals.longest_ms.max(game.longest_session_ms);
                if game.first_played > 0
                    && (totals.first_played == 0 || game.first_played < totals.first_played)
                {
                    totals.first_played = game.first_played;
                }
                totals.last_played = totals.last_played.max(game.last_played);
                for (day, counts) in &game.days {
                    totals.play_ms += counts.play_ms;
                    totals.sessions += u64::from(counts.sessions);
                    totals.launches += u64::from(counts.launches);
                    totals.snapshots += u64::from(counts.snapshots);
                    totals.loads += u64::from(counts.loads);
                    totals.link_ms += counts.link_ms;
                    totals.together_ms += counts.together_ms;
                    for (key, cable) in &counts.cable {
                        let Some(partner) = CablePartner::from_key(key) else {
                            continue;
                        };
                        let name = game.partners.get(key).cloned().unwrap_or_default();
                        add_cable(
                            &mut totals.cable,
                            &CableTotals {
                                partner,
                                name,
                                ms: cable.ms,
                                sessions: u64::from(cable.sessions),
                            },
                        );
                    }
                    overview.fast_forward_ms += counts.fast_forward_ms;
                    overview.rewinds += u64::from(counts.rewinds);
                    if counts.play_ms > 0 {
                        *totals.days.entry(day.clone()).or_default() += counts.play_ms;
                    }
                    if *day == today_key {
                        totals.today_ms += counts.play_ms;
                    }
                    if *day >= week_key && *day <= today_key {
                        totals.week_ms += counts.play_ms;
                    }
                }
                for (day, progress) in &game.progress {
                    let merged = totals.progress.entry(day.clone()).or_default();
                    merged.merge(progress);
                }
            }
        }
        for totals in games.values() {
            overview.play_ms += totals.play_ms;
            overview.today_ms += totals.today_ms;
            overview.week_ms += totals.week_ms;
            overview.sessions += totals.sessions;
            overview.snapshots += totals.snapshots;
            overview.loads += totals.loads;
            overview.link_ms += totals.link_ms;
            overview.together_ms += totals.together_ms;
            for cable in &totals.cable {
                add_cable(&mut overview.cable, cable);
            }
            for (day, ms) in &totals.days {
                *overview.days.entry(day.clone()).or_default() += ms;
            }
        }
        sort_cable(&mut overview.cable);
        for totals in games.values_mut() {
            sort_cable(&mut totals.cable);
        }
        overview.games = games
            .into_values()
            .filter(|g| g.launches > 0 || g.play_ms > 0)
            .collect();
        overview.games_played = overview.games.iter().filter(|g| g.play_ms > 0).count();
        overview.games.sort_by(|a, b| {
            b.play_ms
                .cmp(&a.play_ms)
                .then_with(|| b.last_played.cmp(&a.last_played))
                .then_with(|| a.name.cmp(&b.name))
        });
        overview
    }
}

#[cfg(test)]
mod tests {
    use super::recorder::Recorder;
    use super::*;
    use std::time::Duration;

    const WEDNESDAY: &str = "2026-10-28";
    const THURSDAY: &str = "2026-10-29";

    fn day(key: &str) -> Date {
        parse_day(key).unwrap()
    }

    fn played(stats: &mut Recorder, rom: &str, name: &str, minutes: u64, at: u64, key: &str) {
        stats.launch(rom, name, at, key);
        stats.play(
            rom,
            name,
            Duration::from_secs(minutes * 60),
            Activity::default(),
            at,
            key,
        );
    }

    fn two_days() -> Book {
        let mut stats = Recorder::new(Book::new());
        played(&mut stats, "tetris", "Tetris", 30, 1_000, WEDNESDAY);
        played(
            &mut stats,
            "zelda",
            "Link's Awakening",
            50,
            5_000,
            WEDNESDAY,
        );
        stats.snapshot("zelda", WEDNESDAY);
        stats.chat_message(WEDNESDAY);
        played(
            &mut stats,
            "zelda",
            "Link's Awakening",
            20,
            90_000,
            THURSDAY,
        );
        stats.book().clone()
    }

    #[test]
    fn a_day_lists_what_was_played_that_day() {
        let wednesday = two_days().period(day(WEDNESDAY), day(WEDNESDAY), None);
        assert_eq!(wednesday.play_ms, 80 * 60_000);
        assert_eq!(wednesday.sessions, 2);
        assert_eq!(wednesday.snapshots, 1);
        assert_eq!(wednesday.chat_messages, 1);
        let games: Vec<(&str, u64)> = wednesday
            .games
            .iter()
            .map(|game| (game.name.as_str(), game.play_ms / 60_000))
            .collect();
        assert_eq!(games, [("Link's Awakening", 50), ("Tetris", 30)]);
    }

    #[test]
    fn a_period_adds_up_its_days_and_can_follow_one_game() {
        let book = two_days();
        let both = book.period(day(WEDNESDAY), day(THURSDAY), None);
        assert_eq!(both.play_ms, 100 * 60_000);
        assert_eq!(both.games[0].play_ms, 70 * 60_000);
        let zelda = book.period(day(WEDNESDAY), day(THURSDAY), Some("zelda"));
        assert_eq!(zelda.play_ms, 70 * 60_000);
        assert_eq!(zelda.games.len(), 1);
        assert_eq!(zelda.chat_messages, 0);
        let quiet = book.period(day("2026-10-27"), day("2026-10-27"), None);
        assert_eq!(quiet, Period::default());
    }
}

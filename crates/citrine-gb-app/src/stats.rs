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
            for (day, ms) in &totals.days {
                *overview.days.entry(day.clone()).or_default() += ms;
            }
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

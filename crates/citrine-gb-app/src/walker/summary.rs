use citrine_gb_net::PeerKey;
use citrine_gb_pkm::Gen;
use citrine_gb_pkm::pkm::text;
use citrine_gb_walker::{Shelf, Walker};
use jiff::civil::Date;
use serde::{Deserialize, Serialize};

pub const WEEK: usize = 7;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepsFrom {
    #[default]
    Nothing,
    Phone,
    Game,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Home {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trip {
    #[default]
    Home,
    Borrowed {
        game: String,
    },
    Returning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Left {
    pub key: PeerKey,
    pub chain: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Summary {
    pub pokemon: Option<(String, u8)>,
    pub route: String,
    pub walk_steps: u32,
    pub steps_from: StepsFrom,
    pub watts: u16,
    pub week: Vec<u32>,
    pub caught: u8,
    pub items: u8,
    pub home: Option<Home>,
    pub trip: Trip,
    pub left: Option<Left>,
}

impl Summary {
    pub fn of(walker: &mut Walker, steps_from: StepsFrom) -> Self {
        let pokemon = walker.walking_pokemon().map(|summary| {
            let name = text::decode(Gen::Four, &walker.walking_nickname());
            (name, summary.level)
        });
        let shelves = walker.shelves();
        let count = |pick: fn(&Shelf) -> bool| {
            u8::try_from(shelves.iter().filter(|shelf| pick(shelf)).count()).unwrap_or(u8::MAX)
        };
        Self {
            pokemon,
            route: text::decode(Gen::Four, &walker.route_name()),
            walk_steps: walker.walk_steps(),
            steps_from,
            watts: walker.health().watts(),
            week: week(walker),
            caught: count(|shelf| matches!(shelf, Shelf::Caught(_) | Shelf::EventPokemon)),
            items: count(|shelf| matches!(shelf, Shelf::Item(_) | Shelf::EventItem)),
            home: None,
            trip: Trip::Home,
            left: None,
        }
    }

    pub fn to_label(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn from_label(name: &str, saved: Date, today: Date) -> Option<Self> {
        let mut summary: Self = serde_json::from_str(name).ok()?;
        summary.week = moved_on(&summary.week, saved, today);
        Some(summary)
    }
}

pub fn week(walker: &Walker) -> Vec<u32> {
    let today = walker.today_steps();
    let mut week: Vec<u32> = walker.history().into_iter().take(WEEK - 1).collect();
    week.resize(WEEK - 1, 0);
    week.reverse();
    week.push(today);
    week
}

fn moved_on(week: &[u32], saved: Date, today: Date) -> Vec<u32> {
    let days = saved
        .until(today)
        .map_or(0, |span| span.get_days().max(0) as usize);
    let mut week: Vec<u32> = week.iter().copied().skip(days).collect();
    week.resize(WEEK.max(week.len()), 0);
    let extra = week.len().saturating_sub(WEEK);
    week.drain(..extra);
    week
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb_net::{MAX_LABEL_BYTES, SaveLabel};
    use jiff::civil::date;

    fn full() -> Summary {
        Summary {
            pokemon: Some(("Bisasamxxx".into(), 100)),
            route: "Sattgrüne Wiese Wiese".into(),
            walk_steps: u32::MAX,
            steps_from: StepsFrom::Phone,
            watts: u16::MAX,
            week: vec![u32::MAX; WEEK],
            caught: 3,
            items: 3,
            home: Some(Home {
                id: u64::MAX,
                name: "x".repeat(citrine_gb_net::MAX_DEVICE_NAME_CHARS * 4),
            }),
            trip: Trip::Borrowed {
                game: "SoulSilver".into(),
            },
            left: Some(Left {
                key: citrine_gb_net::RelayKey([u8::MAX; 32]),
                chain: u64::MAX,
            }),
        }
    }

    #[test]
    fn the_largest_summary_fits_a_label() {
        let label = SaveLabel {
            game: String::new(),
            name: full().to_label(),
            device: "x".repeat(citrine_gb_net::MAX_DEVICE_NAME_CHARS * 4),
            saved_at: u64::MAX,
            hash: [0xff; 32],
        };
        let bytes = postcard::to_allocvec(&label).unwrap();
        assert!(bytes.len() + 64 < MAX_LABEL_BYTES, "{} bytes", bytes.len());
    }

    #[test]
    fn a_summary_reads_back_from_its_label() {
        let day = date(2026, 11, 12);
        assert_eq!(
            Summary::from_label(&full().to_label(), day, day),
            Some(full())
        );
    }

    #[test]
    fn a_summary_without_a_home_or_trip_reads_as_at_home() {
        let day = date(2026, 11, 12);
        let read = Summary::from_label(r#"{"route":"Field","walk_steps":5}"#, day, day).unwrap();
        assert_eq!(
            (read.home, read.trip, read.walk_steps),
            (None, Trip::Home, 5)
        );
    }

    #[test]
    fn an_old_label_without_a_summary_reads_as_none() {
        let day = date(2026, 11, 12);
        assert_eq!(Summary::from_label("", day, day), None);
    }

    #[test]
    fn a_summary_from_an_earlier_day_moves_its_week_on() {
        let summary = Summary {
            week: vec![1, 2, 3, 4, 5, 6, 7],
            ..Summary::default()
        };
        let read = Summary::from_label(&summary.to_label(), date(2026, 11, 10), date(2026, 11, 12));
        assert_eq!(read.unwrap().week, [3, 4, 5, 6, 7, 0, 0]);
        let old = Summary::from_label(&summary.to_label(), date(2026, 10, 1), date(2026, 11, 12));
        assert_eq!(old.unwrap().week, [0; WEEK]);
    }

    #[derive(serde::Deserialize, serde::Serialize)]
    struct LabelV0120 {
        game: String,
        name: String,
        device: String,
        saved_at: u64,
        hash: [u8; 32],
    }

    #[test]
    fn a_v0120_client_reads_a_label_with_a_summary() {
        let label = SaveLabel {
            game: String::new(),
            name: full().to_label(),
            device: "Pixel 8".into(),
            saved_at: 1_794_000_000,
            hash: [7; 32],
        };
        let bytes = postcard::to_allocvec(&label).unwrap();
        let old: LabelV0120 = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(
            (old.device.as_str(), old.saved_at, old.hash),
            ("Pixel 8", 1_794_000_000, [7; 32])
        );
    }

    #[test]
    fn this_client_reads_a_v0120_label() {
        let old = LabelV0120 {
            game: String::new(),
            name: String::new(),
            device: "Mac".into(),
            saved_at: 1_794_000_000,
            hash: [7; 32],
        };
        let bytes = postcard::to_allocvec(&old).unwrap();
        let label: SaveLabel = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(label.device, "Mac");
        let day = date(2026, 11, 12);
        assert_eq!(Summary::from_label(&label.name, day, day), None);
    }
}

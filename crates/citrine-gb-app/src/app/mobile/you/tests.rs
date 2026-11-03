use super::*;
use crate::stats::{GameTotals, Overview};

#[test]
fn stats_show_the_week_and_the_most_played_game() {
    let overview = Overview {
        week_ms: 6 * 3_600_000 + 20 * 60_000,
        games: vec![GameTotals {
            name: "Pokémon Silver".into(),
            play_ms: 1,
            ..Default::default()
        }],
        ..Default::default()
    };
    assert_eq!(
        stat_rows(&overview),
        [
            (icons::CHART_BAR, "This week", "6 h 20 min".to_owned()),
            (
                icons::GAME_CONTROLLER,
                "Most played",
                "Pokémon Silver".to_owned()
            )
        ]
    );
}

#[test]
fn no_most_played_before_the_first_game() {
    assert_eq!(stat_rows(&Overview::default()).len(), 1);
}

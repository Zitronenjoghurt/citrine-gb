use super::*;

fn alone() -> TileFacts {
    TileFacts {
        session: false,
        alone: true,
        has_rom: true,
        other_console: None,
        joined: true,
        recognised: false,
        web: false,
    }
}

#[test]
fn screenshot_is_the_first_tile() {
    assert_eq!(tiles(&alone())[0], Tile::Screenshot);
}

#[test]
fn playing_alone_offers_play_together_reset_link_stats_and_close() {
    assert_eq!(
        tiles(&alone()),
        [
            Tile::Screenshot,
            Tile::Record,
            Tile::PlayTogether,
            Tile::Reset,
            Tile::LinkGame,
            Tile::Stats,
            Tile::CloseGame,
        ]
    );
}

#[test]
fn a_friend_session_hides_what_would_end_or_reset_it() {
    let tiles = tiles(&TileFacts {
        session: true,
        alone: false,
        other_console: Some(GbModel::Cgb),
        ..alone()
    });
    for tile in [
        Tile::PlayTogether,
        Tile::Reset,
        Tile::PlayOn(GbModel::Cgb),
        Tile::LinkGame,
        Tile::CloseGame,
    ] {
        assert!(!tiles.contains(&tile), "{tile:?}");
    }
}

#[test]
fn signed_out_has_no_play_together() {
    let tiles = tiles(&TileFacts {
        joined: false,
        ..alone()
    });
    assert!(!tiles.contains(&Tile::PlayTogether));
}

#[test]
fn a_recognised_game_offers_game_info_and_the_web_full_screen() {
    let tiles = tiles(&TileFacts {
        recognised: true,
        web: true,
        other_console: Some(GbModel::Dmg),
        ..alone()
    });
    assert!(tiles.contains(&Tile::GameInfo));
    assert!(tiles.contains(&Tile::PlayOn(GbModel::Dmg)));
    assert_eq!(tiles.last(), Some(&Tile::CloseGame));
    assert!(tiles.contains(&Tile::FullScreen));
}

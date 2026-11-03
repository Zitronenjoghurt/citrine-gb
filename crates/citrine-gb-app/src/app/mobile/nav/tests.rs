use super::*;

fn playing() -> Nav {
    let mut nav = Nav::default();
    nav.show_game();
    nav
}

#[test]
fn back_on_a_game_goes_to_the_games_list_and_never_further_in_one_step() {
    let mut nav = playing();
    assert!(nav.on_game());
    assert!(nav.back());
    assert_eq!(nav.place(), Place::Games);
    assert!(!nav.on_game());
    assert!(!nav.back(), "the games list is the root");
}

#[test]
fn back_closes_the_more_sheet_before_anything_else() {
    let mut nav = playing();
    nav.set_more(true);
    assert!(nav.more_open());
    assert!(!nav.game_in_view());
    assert!(nav.back());
    assert!(nav.on_game());
    assert!(nav.game_in_view());
}

#[test]
fn the_more_sheet_only_opens_over_a_game() {
    let mut nav = Nav::default();
    nav.set_more(true);
    assert!(!nav.more_open());
}

#[test]
fn back_returns_to_the_place_the_user_came_from() {
    let mut nav = playing();
    nav.go(Place::Media);
    nav.go(Place::Settings);
    assert!(nav.back());
    assert_eq!(nav.place(), Place::Media);
    assert!(nav.back());
    assert_eq!(nav.place(), Place::Games);
    assert!(nav.on_game(), "the game is where the user was");
}

#[test]
fn a_place_shows_as_it_was_left() {
    let mut nav = Nav::default();
    nav.open_settings(SettingsTab::Sound);
    nav.go(Place::Friends);
    nav.go(Place::Settings);
    assert_eq!(nav.settings_page(), Some(SettingsTab::Sound));
}

#[test]
fn tapping_the_shown_place_goes_to_its_root() {
    let mut nav = Nav::default();
    nav.open_settings(SettingsTab::Controls);
    nav.go(Place::Settings);
    assert_eq!(nav.settings_page(), None);
    assert!(nav.showing(Tab::Settings));

    let mut nav = playing();
    nav.go(Place::Games);
    assert!(!nav.on_game());
}

#[test]
fn a_page_opens_in_its_place_and_back_pops_it() {
    let mut nav = playing();
    nav.open(Tab::Saves);
    assert_eq!(nav.place(), Place::Games);
    assert!(nav.showing(Tab::Saves));
    assert!(!nav.game_in_view());
    assert!(nav.back());
    assert!(nav.on_game());

    nav.open(Tab::Stats);
    assert_eq!(nav.place(), Place::You);
    assert!(nav.back());
    assert_eq!(nav.place(), Place::You, "stats closes to the profile");
    assert!(nav.back());
    assert!(nav.on_game());
}

#[test]
fn about_is_a_settings_page() {
    let mut nav = Nav::default();
    nav.open(Tab::Info);
    assert_eq!(nav.place(), Place::Settings);
    assert!(nav.showing(Tab::Info));
}

#[test]
fn depth_counts_every_back_step_to_the_root() {
    let mut nav = playing();
    assert_eq!(nav.depth(), 1);
    nav.go(Place::Settings);
    nav.open_settings(SettingsTab::Sound);
    nav.go(Place::Friends);
    let steps = nav.depth();
    let mut walk = nav.clone();
    let mut taken = 0;
    while walk.back() {
        taken += 1;
    }
    assert_eq!(steps, taken);
    assert_eq!(walk.place(), Place::Games);
}

#[test]
fn a_visit_is_listed_once() {
    let mut nav = Nav::default();
    for place in [Place::Media, Place::Settings, Place::Media, Place::Settings] {
        nav.go(place);
    }
    assert!(nav.back());
    assert_eq!(nav.place(), Place::Media);
    assert!(nav.back());
    assert_eq!(nav.place(), Place::Games);
    assert!(!nav.back());
}

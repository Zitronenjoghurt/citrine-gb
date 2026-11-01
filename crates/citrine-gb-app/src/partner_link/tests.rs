use super::{Desk, status};
use citrine_gb_partner::Stage;

#[test]
fn waiting_names_the_desk_for_each_kind_of_trade() {
    let detail = |desk| {
        status(Stage::Waiting, desk, false, "CELEBI")
            .detail
            .unwrap()
    };
    assert!(detail(Desk::Trade).contains("trade desk upstairs"));
    assert!(detail(Desk::TimeCapsule).contains("Time Capsule desk"));
    assert!(detail(Desk::CableClub).contains("Cable Club desk"));
}

#[test]
fn a_finished_trade_shows_until_the_next_pick() {
    let traded = status(Stage::Choosing, Desk::Trade, true, "CELEBI");
    assert_eq!(traded.headline, "Traded");
    assert_eq!(traded.detail.as_deref(), Some("Another CELEBI is ready."));
    assert_eq!(
        status(Stage::Choosing, Desk::Trade, false, "CELEBI").headline,
        "Pick a Pokémon to trade"
    );
}

#[test]
fn confirming_wins_over_an_earlier_trade() {
    assert_eq!(
        status(Stage::Confirming, Desk::Trade, true, "CELEBI").headline,
        "Confirm the trade"
    );
}

#[test]
fn red_and_blue_pick_the_trade_center_after_linking() {
    let linked = status(Stage::Linked, Desk::CableClub, false, "MEW");
    assert!(linked.detail.unwrap().starts_with("Pick Trade Center"));
}

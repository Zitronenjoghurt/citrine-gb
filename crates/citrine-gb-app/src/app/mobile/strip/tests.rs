use super::*;

fn alone() -> StripFacts {
    StripFacts {
        title: "Tobu Tobu Girl".into(),
        session: false,
        room: false,
        watching_only: false,
        linked: false,
        recording: false,
        can_save: true,
    }
}

#[test]
fn playing_alone_puts_fast_forward_pause_and_save_state_before_more() {
    let plan = plan(&alone());
    assert_eq!(plan.lead, StripLead::Title("Tobu Tobu Girl".into()));
    assert_eq!(
        plan.buttons,
        [
            StripButton::Hide,
            StripButton::FastForward,
            StripButton::Pause,
            StripButton::SaveState,
            StripButton::More
        ]
    );
}

#[test]
fn a_friend_session_puts_the_chat_first_and_drops_fast_forward() {
    let plan = plan(&StripFacts {
        session: true,
        ..alone()
    });
    assert_eq!(plan.lead, StripLead::Friend { watching: false });
    assert_eq!(
        plan.buttons,
        [
            StripButton::Hide,
            StripButton::Chat,
            StripButton::Pause,
            StripButton::More
        ]
    );
}

#[test]
fn watching_only_has_no_pause() {
    let plan = plan(&StripFacts {
        session: true,
        watching_only: true,
        ..alone()
    });
    assert_eq!(plan.lead, StripLead::Friend { watching: true });
    assert_eq!(
        plan.buttons,
        [StripButton::Hide, StripButton::Chat, StripButton::More]
    );
}

#[test]
fn two_linked_games_switch_from_the_strip() {
    let plan = plan(&StripFacts {
        linked: true,
        ..alone()
    });
    assert_eq!(
        plan.buttons,
        [
            StripButton::Hide,
            StripButton::Switch,
            StripButton::Pause,
            StripButton::More
        ]
    );
}

#[test]
fn recording_shows_stop_and_hides_fast_forward() {
    let plan = plan(&StripFacts {
        recording: true,
        ..alone()
    });
    assert_eq!(plan.buttons[1], StripButton::StopRecording);
    assert!(!plan.buttons.contains(&StripButton::FastForward));
}

#[test]
fn a_room_keeps_the_game_title_and_leaves_chat_to_the_member_row() {
    let plan = plan(&StripFacts {
        session: true,
        room: true,
        ..alone()
    });
    assert_eq!(plan.lead, StripLead::Title("Tobu Tobu Girl".into()));
    assert_eq!(
        plan.buttons,
        [StripButton::Hide, StripButton::Pause, StripButton::More]
    );
}

#[test]
fn a_linked_room_shows_the_partner_like_a_room_of_two() {
    let plan = plan(&StripFacts {
        session: true,
        room: true,
        linked: true,
        ..alone()
    });
    assert_eq!(plan.lead, StripLead::Friend { watching: false });
    assert_eq!(
        plan.buttons,
        [StripButton::Hide, StripButton::Pause, StripButton::More]
    );
}

#[test]
fn the_strip_takes_a_row_for_each_line_of_buttons() {
    assert_eq!(strip_height(4, 300.0), STRIP_HEIGHT);
    assert_eq!(strip_height(4, 97.0), 2.0 * STRIP_HEIGHT);
    assert_eq!(strip_height(3, 97.0), 2.0 * STRIP_HEIGHT);
    assert_eq!(strip_height(1, 20.0), STRIP_HEIGHT);
}

#[test]
fn the_link_word_matches_the_desktop_and_drops_the_time_first() {
    let width = |text: &str| text.len() as f32;
    assert_eq!(
        link_parts("Slow", 568, 40.0, width),
        ("Slow link".to_owned(), Some(" \u{b7} 568 ms".to_owned()))
    );
    assert_eq!(
        link_parts("Slow", 568, 12.0, width),
        ("Slow link".to_owned(), None)
    );
}

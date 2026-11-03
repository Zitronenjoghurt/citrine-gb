use super::*;

fn alone() -> StripFacts {
    StripFacts {
        title: "Tobu Tobu Girl".into(),
        session: false,
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

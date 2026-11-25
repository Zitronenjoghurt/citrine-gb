use super::needs_copy;
use crate::app::widgets::avatar;
use crate::icons;
use crate::online::Online;
use crate::play::{Ending, IncomingInvite, Play, PlaySession, Stage};
use citrine_gb_net::{GameCard, LinkPath, Mode, OnlineId, Refusal};
use egui::Color32;

const GOOD_MS: u32 = 80;
const OK_MS: u32 = 150;
const ON_DARK: [Color32; 3] = [avatar::ONLINE, avatar::AWAY, avatar::DOT];
const ON_LIGHT: [Color32; 3] = [
    Color32::from_rgb(0x2B, 0x8A, 0x3E),
    Color32::from_rgb(0xA8, 0x62, 0x00),
    Color32::from_rgb(0xC9, 0x2A, 0x3A),
];

pub fn quality(ms: u32, dark: bool) -> (&'static str, Color32) {
    let [good, ok, slow] = if dark { ON_DARK } else { ON_LIGHT };
    if ms < GOOD_MS {
        ("Good", good)
    } else if ms < OK_MS {
        ("OK", ok)
    } else {
        ("Slow", slow)
    }
}

pub fn path_mark(path: LinkPath) -> &'static str {
    match path {
        LinkPath::Direct => icons::ARROWS_HORIZONTAL,
        LinkPath::Relay => icons::GLOBE_SIMPLE,
    }
}

pub fn path_line(path: LinkPath) -> &'static str {
    match path {
        LinkPath::Direct => "Direct connection",
        LinkPath::Relay => "Through the relay",
    }
}

pub fn short_name(display_name: &str) -> &str {
    display_name
        .split_whitespace()
        .next()
        .unwrap_or(display_name)
}

pub fn full_name(play: &Play, online: &Online, who: OnlineId) -> String {
    match play.room().and_then(|r| r.member(who)) {
        Some(member) => member.name.clone(),
        None => online.display_name(who),
    }
}

pub fn member_name(play: &Play, online: &Online, who: OnlineId) -> String {
    short_name(&full_name(play, online, who)).to_owned()
}

pub fn ending_line(name: &str, title: &str, ending: Ending, played: bool) -> Option<String> {
    match ending {
        Ending::Left => None,
        Ending::Declined => Some(format!("{name} declined your invite.")),
        Ending::NoCopy => Some(format!(
            "{name} doesn't have {title}. This relay doesn't share games, so {name} needs their own copy."
        )),
        Ending::OtherCopy => Some(other_copy_line(name, title)),
        Ending::NoAnswer => Some(format!("{name} didn't answer your invite.")),
        Ending::FriendLeft => Some(format!("{name} left.")),
        Ending::Dropped => Some(format!("Lost the connection to {name}.")),
        Ending::Failed if played => Some(format!("Playing with {name} stopped.")),
        Ending::Failed => Some(format!("Couldn't start playing with {name}.")),
        Ending::Moved => Some("You joined from another device.".into()),
    }
}

pub fn failed_line(name: &str, why: &str) -> String {
    format!("Couldn't start playing with {name}. {why}.")
}

pub fn ds_line(title: &str) -> String {
    format!("{title} is a DS game, and DS games can't be played together yet.")
}

pub fn refused_line(name: &str, title: &str, why: Refusal) -> String {
    match why {
        Refusal::NoCopy => format!(
            "You don't have {title}. This relay doesn't share games, so you need your own copy."
        ),
        Refusal::OtherCopy => other_copy_line(name, title),
    }
}

fn other_copy_line(name: &str, title: &str) -> String {
    format!("Your copy of {title} differs from {name}'s, so you can't play together.")
}

const NO_SHARING: &str = " This relay doesn't share games.";

pub fn other_game_line(
    name: &str,
    i_hold_theirs: bool,
    my_title: &str,
    their_title: &str,
    sharing: bool,
    round_trip_ms: Option<u32>,
) -> String {
    let mut watch = if i_hold_theirs {
        format!("{name} doesn't have {my_title}, so you can't watch each other's screens.")
    } else {
        format!("You don't have {their_title}, so you can't watch {name}'s screen.")
    };
    if !sharing {
        watch.push_str(NO_SHARING);
    }
    match round_trip_ms {
        Some(ms) => format!("{watch} Each link cable exchange takes {ms} ms."),
        None => watch,
    }
}

pub fn play_subtitle(play: &Play, friend: OnlineId) -> Option<String> {
    if let Some(session) = play
        .session()
        .filter(|s| s.is_live() && s.friend() == friend)
    {
        let line = match session.stage() {
            Stage::Inviting => "Invite sent",
            _ if needs_copy(session) => return Some(format!("Needs {}", game_title(session))),
            _ if session.watching_only() => "You're watching",
            _ if session.friend_plays() == Some(false) => "Watching you",
            _ => "Playing with you",
        };
        return Some(line.into());
    }
    if let Some(room) = play.room().filter(|r| r.is_live()) {
        if room.inviting().contains(&friend) {
            return Some("Invite sent".into());
        }
        if room.others().any(|w| w == friend) {
            return Some("In the room".into());
        }
    }
    play.invites()
        .iter()
        .find(|i| i.from == friend)
        .map(invite_line)
}

pub fn invite_line(invite: &IncomingInvite) -> String {
    match &invite.game {
        Some(game) => format!("Wants to play {}", card_name(game)),
        None => "Wants to join you".into(),
    }
}

pub fn card_name(card: &GameCard) -> &str {
    crate::game_name::name_or(&card.hash, &card.title)
}

pub fn game_title(session: &PlaySession) -> String {
    session
        .friend_game()
        .or_else(|| session.my_game())
        .map(|g| card_name(g).to_string())
        .unwrap_or_default()
}

pub fn friend_tab_line(name: &str, session: &PlaySession) -> String {
    if session.stage() != Stage::Playing {
        return format!("{name}'s screen shows here once the game starts.");
    }
    if session.friend_picture().is_none() {
        let cable = session.mode() == Some(Mode::Cable);
        return other_game_line(
            name,
            session.holds_theirs().unwrap_or(false),
            session.my_game().map(card_name).unwrap_or_default(),
            &game_title(session),
            session.shares_games(),
            session.round_trip_ms().filter(|_| cable),
        );
    }
    match hidden_line(name, session) {
        Some(line) => format!("{line}."),
        None => format!("{name}'s screen shows here once the game starts."),
    }
}

pub fn hidden_line(name: &str, session: &PlaySession) -> Option<String> {
    if !session.watches_friend() {
        Some(format!("You're not watching {name}"))
    } else if !session.friend_lets_me_watch() {
        Some(format!("{name} isn't showing their screen"))
    } else {
        None
    }
}

pub fn screen_note(name: &str, session: &PlaySession) -> Option<String> {
    if !session.watching_only() {
        return None;
    }
    if session.stage() != Stage::Playing {
        return Some(format!("{name}'s screen shows here once the game starts."));
    }
    if session.friend_plays() != Some(true) {
        return Some(format!("{name} isn't playing anything right now."));
    }
    if session.mode() == Some(Mode::Apart) {
        let mut note = format!("You need {} to watch {name}.", game_title(session));
        if !session.shares_games() {
            note.push_str(NO_SHARING);
        }
        return Some(note);
    }
    if !session.friend_lets_me_watch() {
        return Some(format!("{name} isn't showing their screen."));
    }
    if !session.watches_friend() {
        return Some(format!("You're not watching {name}."));
    }
    None
}

pub fn watched_line(name: &str, session: &PlaySession) -> String {
    if session.mode() == Some(Mode::Apart) {
        format!("{name} needs {} to watch you.", game_title(session))
    } else if session.lets_friend_watch() {
        format!("{}  {name} is watching you", icons::EYE)
    } else {
        format!("{}  {name} can't see your screen", icons::EYE_SLASH)
    }
}

pub fn ask_line(name: &str, invite: &IncomingInvite) -> String {
    match &invite.game {
        Some(game) => format!("{name} wants to play {}", card_name(game)),
        None => format!("{name} wants to join you"),
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use super::{other_game_line, quality, short_name};
    use crate::play::Ending;
    use citrine_gb_net::Refusal;

    #[test]
    fn connection_quality_has_three_words() {
        assert_eq!(quality(42, true).0, "Good");
        assert_eq!(quality(80, true).0, "OK");
        assert_eq!(quality(149, true).0, "OK");
        assert_eq!(quality(150, true).0, "Slow");
    }

    #[test]
    fn the_short_name_is_the_first_word() {
        assert_eq!(short_name("Mia Sommer"), "Mia");
        assert_eq!(short_name("Jonas"), "Jonas");
        assert_eq!(short_name(""), "");
    }

    #[test]
    fn a_ds_game_without_the_same_copy_says_why_on_both_sides() {
        assert_eq!(
            ending_line("Mia", "Pong", Ending::NoCopy, false).as_deref(),
            Some(
                "Mia doesn't have Pong. This relay doesn't share games, so Mia needs their own copy."
            )
        );
        assert_eq!(
            super::refused_line("Mia", "Pong", Refusal::NoCopy),
            "You don't have Pong. This relay doesn't share games, so you need your own copy."
        );
        let differs = "Your copy of Pong differs from Mia's, so you can't play together.";
        assert_eq!(
            ending_line("Mia", "Pong", Ending::OtherCopy, false).as_deref(),
            Some(differs)
        );
        assert_eq!(
            super::refused_line("Mia", "Pong", Refusal::OtherCopy),
            differs
        );
        assert_eq!(
            ending_line("Mia", "Pong", Ending::NoAnswer, false).as_deref(),
            Some("Mia didn't answer your invite.")
        );
    }

    #[test]
    fn the_other_game_line_gives_the_reason_and_a_number() {
        assert_eq!(
            other_game_line("Mia", false, "Petris", "Petris DX", true, Some(238)),
            "You don't have Petris DX, so you can't watch Mia's screen. \
         Each link cable exchange takes 238 ms."
        );
        assert_eq!(
            other_game_line("Mia", true, "Petris", "Petris DX", true, None),
            "Mia doesn't have Petris, so you can't watch each other's screens."
        );
        assert_eq!(
            other_game_line("Mia", false, "Petris", "Petris DX", false, None),
            "You don't have Petris DX, so you can't watch Mia's screen. \
         This relay doesn't share games."
        );
    }
}

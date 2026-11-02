use super::needs_copy;
use crate::app::widgets::avatar;
use crate::icons;
use crate::online::Online;
use crate::play::{Ending, IncomingInvite, Play, PlaySession, Stage};
use citrine_gb_net::{GameCard, Mode, OnlineId};
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

pub fn short_name(display_name: &str) -> &str {
    display_name
        .split_whitespace()
        .next()
        .unwrap_or(display_name)
}

pub fn friend_name(online: &Online, friend: OnlineId) -> String {
    short_name(&online.display_name(friend)).to_owned()
}

pub fn ending_line(name: &str, ending: Ending, played: bool) -> Option<String> {
    match ending {
        Ending::Left => None,
        Ending::Declined => Some(format!("{name} declined your invite.")),
        Ending::FriendLeft => Some(format!("{name} left.")),
        Ending::Dropped => Some(format!("Lost the connection to {name}.")),
        Ending::Failed if played => Some(format!("Playing with {name} stopped.")),
        Ending::Failed => Some(format!("Couldn't start playing with {name}.")),
    }
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

pub fn size_line(got: u32, total: u32) -> String {
    pub(super) const MB: f64 = 1_000_000.0;
    if total as f64 >= MB {
        format!("{:.1} of {:.1} MB", got as f64 / MB, total as f64 / MB)
    } else {
        format!("{} of {} KB", got.div_ceil(1000), total.div_ceil(1000))
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
    play.invites()
        .iter()
        .find(|i| i.from == friend)
        .map(|i| match &i.game {
            Some(game) => format!("Wants to play {}", card_name(game)),
            None => "Wants to join you".into(),
        })
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
    if session.console(false).is_none() {
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

use super::bar::{Switch, bar_switches, switch_missing};
use super::lines::{card_name, short_name};
use crate::online::Online;
use crate::play::{Play, PlaySession, Stage, UNLINK_AFTER};
use citrine_gb_net::{Console, ErrorCode, FriendState, OnlineId, PROTOCOL_VERSION, ROOM_MEMBERS};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    pub who: OnlineId,
    pub name: String,
    pub me: bool,
    pub group: Option<usize>,
    pub reconnecting: bool,
    pub away: bool,
    pub paused: bool,
    pub ms: Option<u16>,
    pub playing: bool,
    pub game: Option<String>,
    pub console: Option<Console>,
    pub can_play_theirs: bool,
    pub friend: Option<FriendState>,
}

impl Person {
    pub fn short(&self) -> &str {
        short_name(&self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupFacts {
    pub authority: OnlineId,
    pub full: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchFacts {
    pub who: OnlineId,
    pub on: bool,
    pub why_off: Option<String>,
    pub missing: bool,
    pub copy: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Waiting {
    pub who: OnlineId,
    pub unlink: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoomFacts {
    pub people: Vec<Person>,
    pub groups: Vec<GroupFacts>,
    pub mine: Option<usize>,
    pub asking: Vec<OnlineId>,
    pub i_play: bool,
    pub watch: Option<WatchFacts>,
    pub rejoining: bool,
    pub unread: usize,
    pub waiting: Option<Waiting>,
    pub watching: Option<OnlineId>,
    pub watch_missing: bool,
    pub hidden: Vec<OnlineId>,
}

impl RoomFacts {
    pub fn of(play: &Play, online: &Online, seen: usize) -> Option<Self> {
        let room = play.room().filter(|r| r.is_live())?;
        if room.members().len() < 2 {
            return None;
        }
        let account = online.account()?;
        let me = account.online_id;
        let view = play.session();
        let person = |who: OnlineId, group: Option<usize>| {
            if who == me {
                return Person {
                    who,
                    name: account.display_name.clone(),
                    me: true,
                    group,
                    reconnecting: room.rejoining(),
                    away: false,
                    paused: false,
                    ms: None,
                    playing: view.is_some_and(|v| v.my_game().is_some()),
                    game: view
                        .and_then(|v| v.my_game())
                        .map(|g| card_name(g).to_owned()),
                    console: view.and_then(|v| v.my_game()).map(|g| g.console),
                    can_play_theirs: false,
                    friend: None,
                };
            }
            let member = room.member(who);
            let theirs = room.pair(who).and_then(|p| p.friend_game());
            Person {
                who,
                name: member.map_or_else(|| online.display_name(who), |m| m.name.clone()),
                me: false,
                group,
                reconnecting: member.is_some_and(|m| m.state.reconnecting),
                away: room.away_members().contains(&who),
                paused: room.paused_members().contains(&who),
                ms: room
                    .pair(who)
                    .and_then(PlaySession::round_trip_ms)
                    .map(|ms| u16::try_from(ms).unwrap_or(u16::MAX))
                    .or_else(|| member.and_then(|m| m.state.round_trip_ms)),
                playing: member.is_some_and(|m| m.state.playing),
                game: theirs.map(|g| card_name(g).to_owned()),
                console: theirs.map(|g| g.console),
                can_play_theirs: theirs.is_some_and(|g| {
                    play.holds(&g.hash) || room.pair(who).is_some_and(PlaySession::shares_games)
                }),
                friend: online.friend(who).map(|f| f.state),
            }
        };
        let mut order: Vec<_> = room.links().groups().iter().collect();
        order.sort_by_key(|g| !g.holds(me));
        let mut facts = Self::default();
        for group in order {
            let index = facts.groups.len();
            facts.groups.push(GroupFacts {
                authority: group.authority(),
                full: group.is_full(),
            });
            if group.holds(me) {
                facts.mine = Some(index);
            }
            for &who in group.members() {
                facts.people.push(person(who, Some(index)));
            }
        }
        for member in room.members() {
            if !facts.people.iter().any(|p| p.who == member.who) {
                facts.people.push(person(member.who, None));
            }
        }
        facts.asking = room.links().asking().to_vec();
        facts.i_play = view.is_some_and(|v| v.my_game().is_some());
        facts.rejoining = room.rejoining();
        facts.waiting = room
            .group_waiting_for(web_time::Instant::now())
            .map(|(who, stalled)| Waiting {
                who,
                unlink: stalled >= UNLINK_AFTER,
            });
        facts.watch = view.and_then(|view| {
            let name = facts
                .people
                .iter()
                .find(|p| p.who == view.friend())
                .map(|p| p.short().to_owned())?;
            let item = bar_switches(view, &name)
                .into_iter()
                .find(|s| s.switch == Switch::Watch)?;
            let missing = switch_missing(view, &item);
            Some(WatchFacts {
                who: view.friend(),
                on: item.on,
                missing,
                copy: missing && view.holds_theirs() != Some(true) && !view.shares_games(),
                why_off: item.why_off,
            })
        });
        facts.watching = room.watching();
        facts.watch_missing = room.watch_missing();
        facts.hidden = room
            .members()
            .iter()
            .filter(|m| !m.state.shows_screen)
            .map(|m| m.who)
            .collect();
        let chat = room.chat();
        facts.unread = chat[seen.min(chat.len())..]
            .iter()
            .filter(|line| line.from != me)
            .count();
        Some(facts)
    }

    pub fn person(&self, who: OnlineId) -> Option<&Person> {
        self.people.iter().find(|p| p.who == who)
    }

    pub fn group_people(&self, group: usize) -> impl Iterator<Item = &Person> {
        self.people.iter().filter(move |p| p.group == Some(group))
    }

    pub fn partners(&self) -> Vec<&Person> {
        match self.mine {
            Some(mine) => self.group_people(mine).filter(|p| !p.me).collect(),
            None => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkEntry {
    Link,
    Play { game: String, with: String },
    Needs(String),
    Join(String),
    Next(String),
    Unlink,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemberMenu {
    pub watch: Option<WatchFacts>,
    pub link: Option<(LinkEntry, OnlineId)>,
    pub moves_me: bool,
    pub message: bool,
    pub profile: bool,
    pub add_friend: bool,
}

pub fn names(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

pub fn member_menu(facts: &RoomFacts, who: OnlineId) -> MemberMenu {
    let Some(person) = facts.person(who) else {
        return MemberMenu::default();
    };
    if person.me {
        return MemberMenu {
            link: facts.mine.map(|_| (LinkEntry::Unlink, who)),
            ..MemberMenu::default()
        };
    }
    let watch = facts
        .watch
        .clone()
        .filter(|w| w.who == who && (person.group.is_none() || person.group == facts.mine))
        .or_else(|| group_watch(facts, person));
    let can_link = facts.i_play && person.playing && !facts.rejoining && !person.reconnecting;
    let gba = |console: Option<Console>| console.map(|c| c == Console::Gba);
    let my_console = facts.people.iter().find(|p| p.me).and_then(|p| p.console);
    let crosses = matches!(
        (gba(my_console), gba(person.console)),
        (Some(mine), Some(theirs)) if mine != theirs
    );
    let game = person.game.clone().unwrap_or_default();
    let link = match person.group {
        _ if !can_link => None,
        Some(group) if Some(group) == facts.mine => Some((LinkEntry::Unlink, who)),
        _ if crosses && !person.can_play_theirs => Some((LinkEntry::Needs(game), who)),
        Some(group) => {
            let with: Vec<&str> = facts
                .group_people(group)
                .filter(|p| p.who != who)
                .map(|p| if p.me { "you" } else { p.short() })
                .collect();
            let with = names(&with);
            let target = &facts.groups[group];
            let entry = if target.full {
                LinkEntry::Next(with)
            } else if crosses {
                LinkEntry::Play { game, with }
            } else {
                LinkEntry::Join(with)
            };
            Some((entry, target.authority))
        }
        None if crosses => Some((
            LinkEntry::Play {
                game,
                with: person.short().to_owned(),
            },
            who,
        )),
        None => Some((LinkEntry::Link, who)),
    };
    let moves_me = facts.mine.is_some()
        && matches!(&link, Some((entry, _)) if !matches!(entry, LinkEntry::Unlink | LinkEntry::Needs(_)));
    MemberMenu {
        watch,
        link,
        moves_me,
        message: person.friend == Some(FriendState::Friend),
        profile: matches!(
            person.friend,
            Some(FriendState::Friend | FriendState::Incoming | FriendState::Outgoing)
        ),
        add_friend: person.friend.is_none(),
    }
}

fn group_watch(facts: &RoomFacts, person: &Person) -> Option<WatchFacts> {
    let group = person.group.filter(|g| Some(*g) != facts.mine)?;
    if facts.group_people(group).count() < 3 {
        return None;
    }
    let on = facts.watching == Some(person.who);
    let hidden = facts.hidden.contains(&person.who);
    Some(WatchFacts {
        who: person.who,
        on: on && !hidden,
        why_off: hidden.then(|| format!("{} isn't showing their screen", person.short())),
        missing: on && facts.watch_missing,
        copy: false,
    })
}

pub fn asks_line(facts: &RoomFacts) -> Option<String> {
    let askers: Vec<&str> = facts
        .asking
        .iter()
        .filter_map(|who| facts.person(*who))
        .map(Person::short)
        .collect();
    match askers.len() {
        0 => None,
        1 => Some(format!("{} wants to link", askers[0])),
        _ => Some(format!("{} want to link", names(&askers))),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Reconnecting,
    Away(String),
    Linked(String, Option<u16>),
}

pub fn status(facts: &RoomFacts) -> Option<Status> {
    if facts.rejoining {
        return Some(Status::Reconnecting);
    }
    let card = wait_card_for(facts);
    let away: Vec<&str> = facts
        .people
        .iter()
        .filter(|p| !p.me && p.reconnecting && Some(p.who) != card)
        .map(Person::short)
        .collect();
    match away.len() {
        0 => {}
        1 => return Some(Status::Away(format!("{} is reconnecting", away[0]))),
        _ => return Some(Status::Away(format!("{} are reconnecting", names(&away)))),
    }
    let partners = facts.partners();
    let told = partners
        .iter()
        .filter(|p| Some(p.who) != card)
        .find_map(|p| match () {
            _ if p.away => Some(format!("{} is away", p.short())),
            _ if p.paused => Some(format!("{} paused", p.short())),
            _ => None,
        });
    if let Some(line) = told {
        return Some(Status::Away(line));
    }
    if let Some(waiting) = facts.waiting.filter(|w| !w.unlink)
        && let Some(person) = facts.person(waiting.who)
    {
        return Some(Status::Away(format!("Waiting for {}", person.short())));
    }
    if facts.waiting.is_some_and(|w| w.unlink) {
        return None;
    }
    if partners.is_empty() {
        return None;
    }
    let with: Vec<&str> = partners.iter().map(|p| p.short()).collect();
    let slowest = partners.iter().filter_map(|p| p.ms).max();
    Some(Status::Linked(
        format!("Linked with {}", names(&with)),
        slowest,
    ))
}

fn wait_card_for(facts: &RoomFacts) -> Option<OnlineId> {
    facts.waiting.filter(|w| w.unlink).map(|w| w.who)
}

pub fn wait_line(facts: &RoomFacts) -> Option<(OnlineId, String, String)> {
    let person = facts.person(wait_card_for(facts)?)?;
    let name = person.short();
    let line = match () {
        _ if person.reconnecting => format!("{name} is reconnecting"),
        _ if person.away => format!("{name} is away"),
        _ if person.paused => format!("{name} paused"),
        _ => format!("Waiting for {name}"),
    };
    Some((person.who, line, format!("Unlink {name}")))
}

pub fn status_text(status: &Status) -> String {
    match status {
        Status::Reconnecting => "Reconnecting".into(),
        Status::Away(line) | Status::Linked(line, _) => line.clone(),
    }
}

pub fn adds_to_room(play: &Play, online: &Online) -> bool {
    online.protocol() >= PROTOCOL_VERSION
        && play
            .room()
            .is_some_and(|r| r.is_live() && r.id().is_some() && !r.rejoining())
        && play
            .session()
            .is_some_and(|s| s.is_live() && s.stage() != Stage::Inviting)
}

pub fn in_room(play: &Play, friend: OnlineId) -> bool {
    play.room()
        .is_some_and(|r| r.others().any(|w| w == friend) || r.inviting().contains(&friend))
}

pub fn note_for(code: ErrorCode, old: &str, failed: bool) -> Option<String> {
    match code {
        ErrorCode::RoomFull => Some("Room is full".into()),
        ErrorCode::NeedsUpdate => Some(format!(
            "Can't invite until {} updates Citrine",
            short_name(old)
        )),
        ErrorCode::CantJoin if failed => Some("Couldn't invite".into()),
        _ => None,
    }
}

pub fn invite_closed(play: &Play, online: &Online, friend: OnlineId) -> bool {
    invite_note(play, online, friend).is_some()
        && play.room().and_then(|r| r.invite_error()) != Some(ErrorCode::CantJoin)
}

pub fn invite_note(play: &Play, online: &Online, friend: OnlineId) -> Option<String> {
    let room = play.room().filter(|r| r.is_live())?;
    if in_room(play, friend) {
        return None;
    }
    let known = match room.old_client() {
        Some(who) => Some((ErrorCode::NeedsUpdate, who)),
        None if room.members().len() + 1 >= ROOM_MEMBERS => Some((ErrorCode::RoomFull, friend)),
        None => None,
    };
    let (code, old) = match (room.invite_error(), known) {
        (Some(code), _) => (code, play.session().map(|s| s.friend())?),
        (None, Some(known)) => known,
        (None, None) => return None,
    };
    let old = room
        .member(old)
        .map_or_else(|| online.display_name(old), |m| m.name.clone());
    note_for(code, &old, room.invite_failed() == Some(friend))
}

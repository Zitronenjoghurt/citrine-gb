use super::{CableByte, GameCard, LinkGroup, Mode, Origin, Pad, PlayMsg, RejoinPoint, Start};
use crate::id::OnlineId;
use crate::proto::GameHash;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct PairStart {
    round: u32,
    mode: Mode,
    seed: u64,
    start_time: u64,
    delay: u8,
    power_on: [u32; 2],
    plugged: bool,
}

#[derive(Serialize, Deserialize)]
enum Wire<'a> {
    Join {
        version: u16,
        round: u32,
        game: Cow<'a, Option<GameCard>>,
    },
    Holds {
        game: GameHash,
        holds: bool,
    },
    Setup {
        round: u32,
        from: Cow<'a, Origin>,
    },
    Ping(u32),
    Pong(u32),
    Start(PairStart),
    Pads {
        round: u32,
        first: u32,
        pads: Cow<'a, [Pad]>,
    },
    Check {
        round: u32,
        frame: u32,
        hashes: [u64; 2],
    },
    Resync {
        round: u32,
        frame: u32,
        unit: u8,
        data: Cow<'a, [u8]>,
    },
    Watch(bool),
    Cable(bool),
    Bytes(Cow<'a, [CableByte]>),
    RoomStart(Cow<'a, Start>),
    UnitPads {
        round: u32,
        unit: u8,
        first: u32,
        pads: Cow<'a, [Pad]>,
    },
    Checks {
        round: u32,
        frame: u32,
        hashes: Cow<'a, [u64]>,
    },
    LinkRequest(bool),
    LinkGroup(Cow<'a, LinkGroup>),
    MemberLeft {
        round: u32,
        unit: u8,
        at_frame: u32,
    },
    Rejoin {
        round: u32,
        from: RejoinPoint,
    },
    Paused(bool),
    WatchGroup(bool),
    Away(bool),
    WatchFrom {
        round: u32,
        frame: u32,
        members: Cow<'a, [OnlineId]>,
        games: Cow<'a, [(u8, GameHash)]>,
        cable: Cow<'a, [u8]>,
    },
}

impl PairStart {
    fn of(start: &Start) -> Option<Self> {
        if start.start_frame != 0 {
            return None;
        }
        Some(Self {
            round: start.round,
            mode: start.mode,
            seed: start.seed,
            start_time: start.start_time,
            delay: start.delay,
            power_on: start.power_on.as_slice().try_into().ok()?,
            plugged: start.plugged,
        })
    }

    fn start(self) -> Start {
        Start {
            round: self.round,
            mode: self.mode,
            seed: self.seed,
            start_time: self.start_time,
            delay: self.delay,
            power_on: self.power_on.to_vec(),
            plugged: self.plugged,
            start_frame: 0,
        }
    }
}

impl<'a> From<&'a PlayMsg> for Wire<'a> {
    fn from(msg: &'a PlayMsg) -> Self {
        match msg {
            PlayMsg::Join {
                version,
                round,
                game,
            } => Wire::Join {
                version: *version,
                round: *round,
                game: Cow::Borrowed(game),
            },
            PlayMsg::Holds { game, holds } => Wire::Holds {
                game: *game,
                holds: *holds,
            },
            PlayMsg::Setup { round, from } => Wire::Setup {
                round: *round,
                from: Cow::Borrowed(from),
            },
            PlayMsg::Ping(n) => Wire::Ping(*n),
            PlayMsg::Pong(n) => Wire::Pong(*n),
            PlayMsg::Start(start) => match PairStart::of(start) {
                Some(pair) => Wire::Start(pair),
                None => Wire::RoomStart(Cow::Borrowed(start)),
            },
            PlayMsg::Pads {
                round,
                unit: None,
                first,
                pads,
            } => Wire::Pads {
                round: *round,
                first: *first,
                pads: Cow::Borrowed(pads),
            },
            PlayMsg::Pads {
                round,
                unit: Some(unit),
                first,
                pads,
            } => Wire::UnitPads {
                round: *round,
                unit: *unit,
                first: *first,
                pads: Cow::Borrowed(pads),
            },
            PlayMsg::Check {
                round,
                frame,
                hashes,
            } => match hashes.as_slice().try_into() {
                Ok(pair) => Wire::Check {
                    round: *round,
                    frame: *frame,
                    hashes: pair,
                },
                Err(_) => Wire::Checks {
                    round: *round,
                    frame: *frame,
                    hashes: Cow::Borrowed(hashes),
                },
            },
            PlayMsg::Resync {
                round,
                frame,
                unit,
                data,
            } => Wire::Resync {
                round: *round,
                frame: *frame,
                unit: *unit,
                data: Cow::Borrowed(data),
            },
            PlayMsg::Watch(on) => Wire::Watch(*on),
            PlayMsg::Cable(on) => Wire::Cable(*on),
            PlayMsg::Bytes(bytes) => Wire::Bytes(Cow::Borrowed(bytes)),
            PlayMsg::LinkRequest(on) => Wire::LinkRequest(*on),
            PlayMsg::LinkGroup(group) => Wire::LinkGroup(Cow::Borrowed(group)),
            PlayMsg::MemberLeft {
                round,
                unit,
                at_frame,
            } => Wire::MemberLeft {
                round: *round,
                unit: *unit,
                at_frame: *at_frame,
            },
            PlayMsg::Rejoin { round, from } => Wire::Rejoin {
                round: *round,
                from: *from,
            },
            PlayMsg::Paused(on) => Wire::Paused(*on),
            PlayMsg::WatchGroup(on) => Wire::WatchGroup(*on),
            PlayMsg::Away(on) => Wire::Away(*on),
            PlayMsg::WatchFrom {
                round,
                frame,
                members,
                games,
                cable,
            } => Wire::WatchFrom {
                round: *round,
                frame: *frame,
                members: Cow::Borrowed(members),
                games: Cow::Borrowed(games),
                cable: Cow::Borrowed(cable),
            },
        }
    }
}

impl From<Wire<'_>> for PlayMsg {
    fn from(wire: Wire<'_>) -> Self {
        match wire {
            Wire::Join {
                version,
                round,
                game,
            } => PlayMsg::Join {
                version,
                round,
                game: game.into_owned(),
            },
            Wire::Holds { game, holds } => PlayMsg::Holds { game, holds },
            Wire::Setup { round, from } => PlayMsg::Setup {
                round,
                from: from.into_owned(),
            },
            Wire::Ping(n) => PlayMsg::Ping(n),
            Wire::Pong(n) => PlayMsg::Pong(n),
            Wire::Start(pair) => PlayMsg::Start(pair.start()),
            Wire::RoomStart(start) => PlayMsg::Start(start.into_owned()),
            Wire::Pads { round, first, pads } => PlayMsg::Pads {
                round,
                unit: None,
                first,
                pads: pads.into_owned(),
            },
            Wire::UnitPads {
                round,
                unit,
                first,
                pads,
            } => PlayMsg::Pads {
                round,
                unit: Some(unit),
                first,
                pads: pads.into_owned(),
            },
            Wire::Check {
                round,
                frame,
                hashes,
            } => PlayMsg::Check {
                round,
                frame,
                hashes: hashes.to_vec(),
            },
            Wire::Checks {
                round,
                frame,
                hashes,
            } => PlayMsg::Check {
                round,
                frame,
                hashes: hashes.into_owned(),
            },
            Wire::Resync {
                round,
                frame,
                unit,
                data,
            } => PlayMsg::Resync {
                round,
                frame,
                unit,
                data: data.into_owned(),
            },
            Wire::Watch(on) => PlayMsg::Watch(on),
            Wire::Cable(on) => PlayMsg::Cable(on),
            Wire::Bytes(bytes) => PlayMsg::Bytes(bytes.into_owned()),
            Wire::LinkRequest(on) => PlayMsg::LinkRequest(on),
            Wire::LinkGroup(group) => PlayMsg::LinkGroup(group.into_owned()),
            Wire::MemberLeft {
                round,
                unit,
                at_frame,
            } => PlayMsg::MemberLeft {
                round,
                unit,
                at_frame,
            },
            Wire::Rejoin { round, from } => PlayMsg::Rejoin { round, from },
            Wire::Paused(on) => PlayMsg::Paused(on),
            Wire::WatchGroup(on) => PlayMsg::WatchGroup(on),
            Wire::Away(on) => PlayMsg::Away(on),
            Wire::WatchFrom {
                round,
                frame,
                members,
                games,
                cable,
            } => PlayMsg::WatchFrom {
                round,
                frame,
                members: members.into_owned(),
                games: games.into_owned(),
                cable: cable.into_owned(),
            },
        }
    }
}

impl Serialize for PlayMsg {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Wire::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PlayMsg {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Wire::deserialize(deserializer).map(PlayMsg::from)
    }
}

#[cfg(test)]
mod tests;

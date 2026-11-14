use super::{
    CableByte, DsUser, GameCard, LinkGroup, Mode, Origin, Pad, PlayMsg, RejoinPoint, Start,
};
use crate::id::OnlineId;
use crate::proto::GameHash;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(super) struct PairStart {
    round: u32,
    mode: Mode,
    seed: u64,
    start_time: u64,
    delay: u8,
    power_on: [u32; 2],
    plugged: bool,
}

#[derive(Serialize, Deserialize)]
pub(super) enum Wire<'a> {
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
    Got {
        game: GameHash,
        upto: u32,
    },
    CableResync {
        round: u32,
        frame: u32,
        unit: u8,
        data: Cow<'a, [u8]>,
        cable: Cow<'a, [u8]>,
    },
    Piece {
        id: u32,
        offset: u32,
        total: u32,
        data: Cow<'a, [u8]>,
    },
    DsPads {
        round: u32,
        unit: Option<u8>,
        first: u32,
        pads: Vec<DsPad>,
    },
    DsUser {
        round: u32,
        user: Cow<'a, DsUser>,
    },
    Delay {
        round: u32,
        delay: u8,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(super) struct DsPad {
    buttons: u16,
    cable: Option<bool>,
    touch: Option<[u8; 2]>,
}

impl From<&Pad> for DsPad {
    fn from(pad: &Pad) -> Self {
        Self {
            buttons: pad.buttons,
            cable: pad.cable,
            touch: pad.touch,
        }
    }
}

impl From<DsPad> for Pad {
    fn from(pad: DsPad) -> Self {
        Self {
            buttons: pad.buttons,
            cable: pad.cable,
            touch: pad.touch,
        }
    }
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
                unit,
                first,
                pads,
            } if pads.iter().any(|pad| pad.touch.is_some()) => Wire::DsPads {
                round: *round,
                unit: *unit,
                first: *first,
                pads: pads.iter().map(DsPad::from).collect(),
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
                cable: Some(cable),
            } => Wire::CableResync {
                round: *round,
                frame: *frame,
                unit: *unit,
                data: Cow::Borrowed(data),
                cable: Cow::Borrowed(cable),
            },
            PlayMsg::Resync {
                round,
                frame,
                unit,
                data,
                cable: None,
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
            PlayMsg::Got { game, upto } => Wire::Got {
                game: *game,
                upto: *upto,
            },
            PlayMsg::DsUser { round, user } => Wire::DsUser {
                round: *round,
                user: Cow::Borrowed(user),
            },
            PlayMsg::Delay { round, delay } => Wire::Delay {
                round: *round,
                delay: *delay,
            },
        }
    }
}

impl Wire<'_> {
    pub(super) fn into_msg(self) -> Option<PlayMsg> {
        Some(match self {
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
                cable: None,
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
            Wire::Got { game, upto } => PlayMsg::Got { game, upto },
            Wire::CableResync {
                round,
                frame,
                unit,
                data,
                cable,
            } => PlayMsg::Resync {
                round,
                frame,
                unit,
                data: data.into_owned(),
                cable: Some(cable.into_owned()),
            },
            Wire::DsPads {
                round,
                unit,
                first,
                pads,
            } => PlayMsg::Pads {
                round,
                unit,
                first,
                pads: pads.into_iter().map(Pad::from).collect(),
            },
            Wire::DsUser { round, user } => PlayMsg::DsUser {
                round,
                user: user.into_owned(),
            },
            Wire::Delay { round, delay } => PlayMsg::Delay { round, delay },
            Wire::Piece { .. } => return None,
        })
    }
}

impl Serialize for PlayMsg {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Wire::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PlayMsg {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Wire::deserialize(deserializer)?
            .into_msg()
            .ok_or_else(|| serde::de::Error::custom("a piece of a message"))
    }
}

#[cfg(test)]
mod tests {
    use super::super::Console;
    use super::*;
    use crate::id::OnlineId;

    #[derive(Serialize)]
    enum Before {
        Join {
            version: u16,
            round: u32,
            game: Option<GameCard>,
        },
        Holds {
            game: GameHash,
            holds: bool,
        },
        Setup {
            round: u32,
            from: Origin,
        },
        Ping(u32),
        Pong(u32),
        Start(PairStart),
        Pads {
            round: u32,
            first: u32,
            pads: Vec<BeforePad>,
        },
        Check {
            round: u32,
            frame: u32,
            hashes: [u64; 2],
        },
        Resync {
            round: u32,
            frame: u32,
            player: u8,
            data: Vec<u8>,
        },
        Watch(bool),
        Cable(bool),
        Bytes(Vec<CableByte>),
    }

    #[derive(Serialize)]
    struct BeforePad {
        buttons: u8,
        cable: Option<bool>,
    }

    fn card() -> GameCard {
        GameCard {
            hash: [7; 20],
            size: 32 * 1024,
            title: "Petris".into(),
            console: crate::Console::Gbc,
        }
    }

    fn pads() -> Vec<Pad> {
        vec![
            Pad {
                buttons: 0x381,
                cable: None,
                touch: None,
            },
            Pad {
                buttons: 0,
                cable: Some(true),
                touch: None,
            },
        ]
    }

    fn pair_start() -> PairStart {
        PairStart {
            round: 2,
            mode: Mode::Rollback,
            seed: 1,
            start_time: 1_790_000_000,
            delay: 2,
            power_on: [0, 60],
            plugged: true,
        }
    }

    fn pairs() -> Vec<(Before, PlayMsg)> {
        vec![
            (
                Before::Join {
                    version: 3,
                    round: 2,
                    game: Some(card()),
                },
                PlayMsg::Join {
                    version: 3,
                    round: 2,
                    game: Some(card()),
                },
            ),
            (
                Before::Holds {
                    game: [1; 20],
                    holds: true,
                },
                PlayMsg::Holds {
                    game: [1; 20],
                    holds: true,
                },
            ),
            (
                Before::Setup {
                    round: 3,
                    from: Origin::Resume(vec![4; 300]),
                },
                PlayMsg::Setup {
                    round: 3,
                    from: Origin::Resume(vec![4; 300]),
                },
            ),
            (Before::Ping(9), PlayMsg::Ping(9)),
            (Before::Pong(9), PlayMsg::Pong(9)),
            (
                Before::Start(pair_start()),
                PlayMsg::Start(Start {
                    round: 2,
                    mode: Mode::Rollback,
                    seed: 1,
                    start_time: 1_790_000_000,
                    delay: 2,
                    power_on: vec![0, 60],
                    plugged: true,
                    start_frame: 0,
                }),
            ),
            (
                Before::Pads {
                    round: 2,
                    first: 300,
                    pads: vec![
                        BeforePad {
                            buttons: 0x21,
                            cable: None,
                        },
                        BeforePad {
                            buttons: 0,
                            cable: Some(true),
                        },
                    ],
                },
                PlayMsg::Pads {
                    round: 2,
                    unit: None,
                    first: 300,
                    pads: vec![
                        Pad {
                            buttons: 0x21,
                            cable: None,
                            touch: None,
                        },
                        Pad {
                            buttons: 0,
                            cable: Some(true),
                            touch: None,
                        },
                    ],
                },
            ),
            (
                Before::Check {
                    round: 2,
                    frame: 60,
                    hashes: [1, u64::MAX],
                },
                PlayMsg::Check {
                    round: 2,
                    frame: 60,
                    hashes: vec![1, u64::MAX],
                },
            ),
            (
                Before::Resync {
                    round: 2,
                    frame: 120,
                    player: 1,
                    data: vec![5; 100],
                },
                PlayMsg::Resync {
                    round: 2,
                    frame: 120,
                    unit: 1,
                    data: vec![5; 100],
                    cable: None,
                },
            ),
            (Before::Watch(true), PlayMsg::Watch(true)),
            (Before::Cable(false), PlayMsg::Cable(false)),
            (
                Before::Bytes(vec![CableByte::Reply { id: 1, byte: 0xFE }]),
                PlayMsg::Bytes(vec![CableByte::Reply { id: 1, byte: 0xFE }]),
            ),
        ]
    }

    #[test]
    fn every_message_of_play_version_three_decodes_to_the_same_meaning() {
        for (before, now) in pairs() {
            let bytes = postcard::to_allocvec(&before).unwrap();
            assert_eq!(postcard::from_bytes::<PlayMsg>(&bytes).unwrap(), now);
        }
    }

    #[test]
    fn two_player_messages_keep_the_bytes_of_play_version_three() {
        for (before, now) in pairs() {
            assert_eq!(
                postcard::to_allocvec(&now).unwrap(),
                postcard::to_allocvec(&before).unwrap(),
                "{now:?}"
            );
        }
    }

    #[test]
    fn room_messages_take_new_tags_after_the_old_ones() {
        let start = Start {
            power_on: vec![0, 60, 120],
            ..pair_start().start()
        };
        let resumed = Start {
            start_frame: 9_000,
            ..pair_start().start()
        };
        let msgs = [
            (12, PlayMsg::Start(start)),
            (12, PlayMsg::Start(resumed)),
            (
                13,
                PlayMsg::Pads {
                    round: 1,
                    unit: Some(2),
                    first: 7,
                    pads: pads(),
                },
            ),
            (
                14,
                PlayMsg::Check {
                    round: 1,
                    frame: 60,
                    hashes: vec![1, 2, 3, 4],
                },
            ),
            (15, PlayMsg::LinkRequest(true)),
            (
                16,
                PlayMsg::LinkGroup(LinkGroup {
                    round: 1,
                    at_frame: 600,
                    members: vec![OnlineId::random(), OnlineId::random()],
                    plugged: true,
                    console: Some(Console::Gba),
                    watchers: Vec::new(),
                }),
            ),
            (
                17,
                PlayMsg::MemberLeft {
                    round: 1,
                    unit: 3,
                    at_frame: 900,
                },
            ),
            (19, PlayMsg::Paused(true)),
            (20, PlayMsg::WatchGroup(true)),
            (21, PlayMsg::Away(true)),
            (
                22,
                PlayMsg::WatchFrom {
                    round: 1 << 31,
                    frame: 300,
                    members: vec![OnlineId::random(), OnlineId::random(), OnlineId::random()],
                    games: vec![(1, [9; 20])],
                    cable: vec![0; 8],
                },
            ),
            (
                23,
                PlayMsg::Got {
                    game: [6; 20],
                    upto: 3 << 20,
                },
            ),
            (
                26,
                PlayMsg::Pads {
                    round: 2,
                    unit: Some(1),
                    first: 40,
                    pads: vec![
                        Pad {
                            buttons: 0x1001,
                            cable: None,
                            touch: Some([128, 96]),
                        },
                        Pad::default(),
                    ],
                },
            ),
            (
                26,
                PlayMsg::Pads {
                    round: 2,
                    unit: None,
                    first: 41,
                    pads: vec![Pad {
                        buttons: 0,
                        cable: Some(true),
                        touch: Some([0, 191]),
                    }],
                },
            ),
            (
                27,
                PlayMsg::DsUser {
                    round: 2,
                    user: DsUser {
                        nickname: "Clara".into(),
                        message: "hi".into(),
                        color: 11,
                        birthday: (10, 25),
                        language: 2,
                        mac: [0x00, 0x09, 0xBF, 1, 2, 3],
                    },
                },
            ),
            (
                28,
                PlayMsg::Delay {
                    round: 2,
                    delay: 10,
                },
            ),
        ];
        for (tag, msg) in msgs {
            let bytes = postcard::to_allocvec(&msg).unwrap();
            assert_eq!(bytes[0], tag, "{msg:?}");
            assert_eq!(postcard::from_bytes::<PlayMsg>(&bytes).unwrap(), msg);
        }
    }

    #[test]
    fn a_check_of_one_unit_takes_the_room_form() {
        let msg = PlayMsg::Check {
            round: 1,
            frame: 60,
            hashes: vec![5],
        };
        let bytes = postcard::to_allocvec(&msg).unwrap();
        assert_eq!(bytes, [14, 1, 60, 1, 5]);
        assert_eq!(postcard::from_bytes::<PlayMsg>(&bytes).unwrap(), msg);
    }

    #[test]
    fn pads_carry_l_and_r_where_version_four_cut_them_off() {
        let pad = Pad {
            buttons: 0x300 | 0x81,
            cable: None,
            touch: None,
        };
        let msg = PlayMsg::Pads {
            round: 1,
            unit: None,
            first: 0,
            pads: vec![pad],
        };
        let bytes = postcard::to_allocvec(&msg).unwrap();
        assert_eq!(postcard::from_bytes::<PlayMsg>(&bytes).unwrap(), msg);
        let before = Before::Pads {
            round: 1,
            first: 0,
            pads: vec![BeforePad {
                buttons: 0x81,
                cable: None,
            }],
        };
        assert_ne!(postcard::to_allocvec(&before).unwrap(), bytes);
    }
}

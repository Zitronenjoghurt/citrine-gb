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
        },
        Pad {
            buttons: 0,
            cable: Some(true),
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
                    },
                    Pad {
                        buttons: 0,
                        cable: Some(true),
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

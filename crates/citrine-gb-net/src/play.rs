mod wire;

use crate::crypto::{Keys, OpenError, PublicKey, Sealed};
use crate::id::OnlineId;
use crate::proto::{GameHash, SessionId};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub const PLAY_VERSION: u16 = 5;
pub const OLDEST_PLAY_VERSION: u16 = 5;
pub const MAX_UNITS: usize = 4;
pub const GAME_PART_BYTES: usize = 192 * 1024;
pub const MAX_GAME_BYTES: u32 = 8 << 20;
pub const MAX_TITLE_CHARS: usize = 64;
pub const MAX_STATE_BYTES: usize = 2 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Console {
    Gb,
    Gbc,
    Gba,
}

pub fn plays_with(version: u16) -> bool {
    (OLDEST_PLAY_VERSION..=PLAY_VERSION).contains(&version)
}

impl Console {
    fn from_wire(index: u32) -> Option<Self> {
        match index {
            0 => Some(Console::Gb),
            1 => Some(Console::Gbc),
            2 => Some(Console::Gba),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OfferError {
    #[error(transparent)]
    Sealed(#[from] OpenError),
    #[error("the offer is for a console this build doesn't know")]
    UnknownConsole,
    #[error("the offer asks for something this build doesn't know")]
    UnknownAsk,
}

#[derive(Deserialize)]
struct WireOffer {
    version: u16,
    ask: u32,
    game: Option<WireCard>,
}

#[derive(Deserialize)]
struct WireCard {
    hash: GameHash,
    size: u32,
    title: String,
    console: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ask {
    Play,
}

impl Ask {
    fn from_wire(index: u32) -> Option<Self> {
        match index {
            0 => Some(Ask::Play),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameCard {
    pub hash: GameHash,
    pub size: u32,
    pub title: String,
    pub console: Console,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offer {
    pub version: u16,
    pub ask: Ask,
    pub game: Option<GameCard>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    Rollback,
    Cable,
    Apart,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Start {
    pub round: u32,
    pub mode: Mode,
    pub seed: u64,
    pub start_time: u64,
    pub delay: u8,
    pub power_on: Vec<u32>,
    pub plugged: bool,
    pub start_frame: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Pad {
    pub buttons: u16,
    pub cable: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CableByte {
    Transfer {
        id: u16,
        byte: u8,
        seq: u16,
        since: u32,
    },
    Reply {
        id: u16,
        byte: u8,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Origin {
    Boot(Option<Vec<u8>>),
    Resume(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkGroup {
    pub round: u32,
    pub at_frame: u32,
    pub members: Vec<OnlineId>,
    pub plugged: bool,
    pub console: Option<Console>,
    pub watchers: Vec<OnlineId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayMsg {
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
    Start(Start),
    Pads {
        round: u32,
        unit: Option<u8>,
        first: u32,
        pads: Vec<Pad>,
    },
    Check {
        round: u32,
        frame: u32,
        hashes: Vec<u64>,
    },
    Resync {
        round: u32,
        frame: u32,
        unit: u8,
        data: Vec<u8>,
    },
    Watch(bool),
    Cable(bool),
    Bytes(Vec<CableByte>),
    LinkRequest(bool),
    LinkGroup(LinkGroup),
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
        members: Vec<OnlineId>,
        games: Vec<(u8, GameHash)>,
        cable: Vec<u8>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejoinPoint {
    pub session: SessionId,
    pub round: u32,
    pub frame: u32,
    pub oldest: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GamePart {
    pub hash: GameHash,
    pub offset: u32,
    pub total: u32,
    pub data: Vec<u8>,
}

fn seal<T: Serialize>(value: &T, keys: &Keys, to: &PublicKey) -> Sealed {
    let bytes = postcard::to_allocvec(value).expect("session messages always encode");
    keys.seal(to, &bytes)
}

fn open<T: DeserializeOwned>(
    keys: &Keys,
    from: &PublicKey,
    sealed: &Sealed,
) -> Result<T, OpenError> {
    let bytes = keys.open(from, sealed)?;
    match postcard::take_from_bytes(&bytes) {
        Ok((value, [])) => Ok(value),
        _ => Err(OpenError),
    }
}

impl Offer {
    pub fn seal(&self, keys: &Keys, to: &PublicKey) -> Sealed {
        seal(self, keys, to)
    }

    pub fn open(keys: &Keys, from: &PublicKey, sealed: &Sealed) -> Result<Self, OfferError> {
        let wire: WireOffer = open(keys, from, sealed)?;
        let ask = Ask::from_wire(wire.ask).ok_or(OfferError::UnknownAsk)?;
        let game = match wire.game {
            Some(card) => Some(GameCard {
                hash: card.hash,
                size: card.size,
                title: card.title,
                console: Console::from_wire(card.console).ok_or(OfferError::UnknownConsole)?,
            }),
            None => None,
        };
        Ok(Offer {
            version: wire.version,
            ask,
            game,
        })
    }
}

impl PlayMsg {
    pub fn seal(&self, keys: &Keys, to: &PublicKey) -> Sealed {
        seal(self, keys, to)
    }

    pub fn open(keys: &Keys, from: &PublicKey, sealed: &Sealed) -> Result<Self, OpenError> {
        open(keys, from, sealed)
    }
}

impl GamePart {
    pub fn seal(&self, keys: &Keys, to: &PublicKey) -> Sealed {
        seal(self, keys, to)
    }

    pub fn open(keys: &Keys, from: &PublicKey, sealed: &Sealed) -> Result<Self, OpenError> {
        open(keys, from, sealed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClientMsg, MAX_FRAME_BYTES, RelayId};

    fn pair() -> (Keys, Keys) {
        (RelayId::random().keys(), RelayId::random().keys())
    }

    fn card() -> GameCard {
        GameCard {
            hash: [7; 20],
            size: 32 * 1024,
            title: "Petris".into(),
            console: Console::Gbc,
        }
    }

    #[test]
    fn every_play_message_crosses_sealed() {
        let (a, b) = pair();
        let msgs = [
            PlayMsg::Join {
                version: PLAY_VERSION,
                round: 2,
                game: Some(card()),
            },
            PlayMsg::Join {
                version: PLAY_VERSION,
                round: 3,
                game: None,
            },
            PlayMsg::Holds {
                game: [1; 20],
                holds: true,
            },
            PlayMsg::Setup {
                round: 2,
                from: Origin::Boot(Some(vec![3; 8 * 1024])),
            },
            PlayMsg::Setup {
                round: 2,
                from: Origin::Boot(None),
            },
            PlayMsg::Setup {
                round: 3,
                from: Origin::Resume(vec![4; 60 * 1024]),
            },
            PlayMsg::Ping(9),
            PlayMsg::Pong(9),
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
            PlayMsg::Start(Start {
                round: 4,
                mode: Mode::Rollback,
                seed: 1,
                start_time: 1_790_000_000,
                delay: 0,
                power_on: vec![0, 0, 60, 120],
                plugged: false,
                start_frame: 36_000,
            }),
            PlayMsg::Pads {
                round: 2,
                unit: None,
                first: 300,
                pads: vec![
                    Pad {
                        buttons: 0x81,
                        cable: None,
                    },
                    Pad {
                        buttons: 0,
                        cable: Some(false),
                    },
                ],
            },
            PlayMsg::Check {
                round: 2,
                frame: 60,
                hashes: vec![1, u64::MAX],
            },
            PlayMsg::Pads {
                round: 2,
                unit: Some(3),
                first: 300,
                pads: vec![Pad {
                    buttons: 0x10,
                    cable: Some(true),
                }],
            },
            PlayMsg::Check {
                round: 2,
                frame: 60,
                hashes: vec![1, 2, u64::MAX],
            },
            PlayMsg::Resync {
                round: 2,
                frame: 120,
                unit: 1,
                data: vec![5; 1000],
            },
            PlayMsg::Resync {
                round: 2,
                frame: 120,
                unit: 3,
                data: vec![6; 1000],
            },
            PlayMsg::Watch(true),
            PlayMsg::Cable(false),
            PlayMsg::Bytes(vec![
                CableByte::Transfer {
                    id: 1,
                    byte: 0x60,
                    seq: 2,
                    since: 70_000,
                },
                CableByte::Reply { id: 1, byte: 0xFE },
            ]),
            PlayMsg::LinkRequest(true),
            PlayMsg::LinkRequest(false),
            PlayMsg::LinkGroup(LinkGroup {
                round: 2,
                at_frame: 600,
                members: vec![OnlineId::random(), OnlineId::random(), OnlineId::random()],
                plugged: true,
                console: Some(Console::Gba),
                watchers: vec![OnlineId::random()],
            }),
            PlayMsg::MemberLeft {
                round: 2,
                unit: 2,
                at_frame: 1_200,
            },
            rejoin(),
            PlayMsg::Paused(true),
            PlayMsg::WatchGroup(false),
            PlayMsg::Away(true),
            watch_from(),
        ];
        for msg in msgs {
            let sealed = msg.seal(&a, &b.public_key());
            assert_eq!(PlayMsg::open(&b, &a.public_key(), &sealed), Ok(msg));
        }
    }

    fn watch_from() -> PlayMsg {
        PlayMsg::WatchFrom {
            round: 1 << 31,
            frame: 640,
            members: vec![OnlineId::random(), OnlineId::random(), OnlineId::random()],
            games: vec![(0, [3; 20]), (2, [4; 20])],
            cable: vec![1, 2, 3],
        }
    }

    fn rejoin() -> PlayMsg {
        PlayMsg::Rejoin {
            round: 1,
            from: RejoinPoint {
                session: 77,
                round: 3,
                frame: 905,
                oldest: 880,
            },
        }
    }

    #[test]
    fn a_rejoin_point_keeps_the_older_messages_where_they_were() {
        let tag = |msg: &PlayMsg| postcard::to_allocvec(msg).unwrap()[0];
        let left = PlayMsg::MemberLeft {
            round: 0,
            unit: 0,
            at_frame: 0,
        };
        assert_eq!(tag(&left), 17);
        assert_eq!(tag(&rejoin()), 18);
        assert_eq!(tag(&PlayMsg::Paused(true)), 19);
        assert_eq!(tag(&PlayMsg::WatchGroup(true)), 20);
        assert_eq!(tag(&PlayMsg::Away(true)), 21);
        assert_eq!(tag(&watch_from()), 22);
    }

    #[test]
    fn a_stranger_cannot_open_a_session_message() {
        let (a, b) = pair();
        let (c, _) = pair();
        let sealed = PlayMsg::Watch(true).seal(&a, &b.public_key());
        assert_eq!(PlayMsg::open(&c, &a.public_key(), &sealed), Err(OpenError));
        let offer = Offer {
            version: PLAY_VERSION,
            ask: Ask::Play,
            game: Some(card()),
        };
        let sealed = offer.seal(&a, &b.public_key());
        assert_eq!(Offer::open(&b, &a.public_key(), &sealed), Ok(offer));
        assert_eq!(
            Offer::open(&c, &a.public_key(), &sealed),
            Err(OfferError::Sealed(OpenError))
        );
    }

    #[derive(Serialize)]
    struct FutureOffer {
        version: u16,
        ask: FutureAsk,
        game: Option<FutureCard>,
    }

    #[derive(Serialize)]
    enum FutureAsk {
        Play,
        Watch,
        Show,
        Trade,
    }

    #[derive(Serialize)]
    struct FutureCard {
        hash: GameHash,
        size: u32,
        title: String,
        console: FutureConsole,
    }

    #[derive(Serialize)]
    enum FutureConsole {
        Gb,
        Gbc,
        Gba,
        Nds,
    }

    #[test]
    fn an_offer_for_an_unknown_console_says_so() {
        let (a, b) = pair();
        let open = |console| {
            let offer = FutureOffer {
                version: PLAY_VERSION,
                ask: FutureAsk::Play,
                game: Some(FutureCard {
                    hash: [7; 20],
                    size: 32 * 1024,
                    title: "Petris".into(),
                    console,
                }),
            };
            let sealed = seal(&offer, &a, &b.public_key());
            Offer::open(&b, &a.public_key(), &sealed).map(|offer| offer.game.map(|g| g.console))
        };
        assert_eq!(open(FutureConsole::Gb), Ok(Some(Console::Gb)));
        assert_eq!(open(FutureConsole::Gbc), Ok(Some(Console::Gbc)));
        assert_eq!(open(FutureConsole::Gba), Ok(Some(Console::Gba)));
        let err = open(FutureConsole::Nds).unwrap_err();
        assert_eq!(err, OfferError::UnknownConsole);
        assert_eq!(
            err.to_string(),
            "the offer is for a console this build doesn't know"
        );
    }

    #[test]
    fn one_invite_crosses_with_or_without_a_game() {
        let (a, b) = pair();
        for game in [Some(card()), None] {
            let offer = Offer {
                version: PLAY_VERSION,
                ask: Ask::Play,
                game,
            };
            let sealed = offer.seal(&a, &b.public_key());
            assert_eq!(Offer::open(&b, &a.public_key(), &sealed), Ok(offer));
        }
        for ask in [FutureAsk::Watch, FutureAsk::Show, FutureAsk::Trade] {
            let future = FutureOffer {
                version: PLAY_VERSION,
                ask,
                game: None,
            };
            let sealed = seal(&future, &a, &b.public_key());
            let err = Offer::open(&b, &a.public_key(), &sealed).unwrap_err();
            assert_eq!(err, OfferError::UnknownAsk);
        }
    }

    #[test]
    fn every_known_console_opens_from_an_offer() {
        let (a, b) = pair();
        for console in [Console::Gb, Console::Gbc, Console::Gba] {
            let offer = Offer {
                version: PLAY_VERSION,
                ask: Ask::Play,
                game: Some(GameCard { console, ..card() }),
            };
            let sealed = offer.seal(&a, &b.public_key());
            assert_eq!(Offer::open(&b, &a.public_key(), &sealed), Ok(offer));
        }
    }

    #[test]
    fn a_game_part_fits_in_a_frame() {
        let (a, b) = pair();
        let part = GamePart {
            hash: [9; 20],
            offset: 0,
            total: MAX_GAME_BYTES,
            data: vec![0xFF; GAME_PART_BYTES],
        };
        let sealed = part.seal(&a, &b.public_key());
        let msg = ClientMsg::Game {
            session: u64::MAX,
            sealed: sealed.clone(),
        };
        assert!(msg.encode().unwrap().len() < MAX_FRAME_BYTES);
        assert_eq!(GamePart::open(&b, &a.public_key(), &sealed), Ok(part));
    }

    #[test]
    fn one_frame_of_input_stays_small() {
        let (a, b) = pair();
        let sealed = PlayMsg::Pads {
            round: 1_000,
            unit: None,
            first: 1_000_000,
            pads: vec![Pad::default()],
        }
        .seal(&a, &b.public_key());
        let msg = ClientMsg::Play {
            session: u64::MAX,
            sealed,
        };
        assert!(msg.encode().unwrap().len() <= 72);
    }

    #[test]
    fn this_build_plays_only_with_play_version_five() {
        assert!(!plays_with(3));
        assert!(!plays_with(4));
        assert!(plays_with(5));
        assert!(!plays_with(6));
    }
}

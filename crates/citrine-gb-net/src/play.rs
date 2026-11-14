mod pieces;
mod wire;

use crate::chat::{pack, unpack};
use crate::crypto::{Keys, OpenError, PeerSealer, PublicKey, Sealed};
use crate::id::OnlineId;
use crate::proto::{GameHash, SessionId};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub use pieces::{MAX_PIECED_BYTES, PIECE_BYTES, Pieces, WHOLE_BYTES};

pub const PLAY_VERSION: u16 = 13;
pub const OLDEST_PLAY_VERSION: u16 = 13;
pub const MAX_UNITS: usize = 4;
pub const GAME_PART_BYTES: usize = 192 * 1024;
const GAME_SAMPLE_BYTES: usize = 16 * 1024;
pub const MAX_GAME_BYTES: u32 = 32 << 20;
pub const MAX_DS_GAME_BYTES: u32 = 512 << 20;
pub const UNPACED_GAME_BYTES: u32 = 8 << 20;
pub const GAME_ACK_BYTES: u32 = 1 << 20;
pub const GAME_WINDOW_BYTES: u32 = 2 << 20;
pub const MAX_TITLE_CHARS: usize = 64;
const NDS_WIRE: u32 = 3;
pub const MAX_STATE_BYTES: usize = 2 << 20;
pub const MAX_DS_STATE_BYTES: usize = 16 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Console {
    Gb,
    Gbc,
    Gba,
    Nds { code: [u8; 4] },
}

pub fn plays_with(version: u16) -> bool {
    (OLDEST_PLAY_VERSION..=PLAY_VERSION).contains(&version)
}

impl Console {
    pub fn max_state_bytes(self) -> usize {
        match self {
            Self::Gb | Self::Gbc | Self::Gba => MAX_STATE_BYTES,
            Self::Nds { .. } => MAX_DS_STATE_BYTES,
        }
    }

    pub fn is_ds(self) -> bool {
        matches!(self, Self::Nds { .. })
    }

    pub fn same_ds_game(self, other: Console) -> bool {
        match (self, other) {
            (Self::Nds { code: a }, Self::Nds { code: b }) => a[..3] == b[..3],
            _ => false,
        }
    }

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
    console: WireConsole,
}

enum WireConsole {
    Known(Console),
    Unknown,
}

impl<'de> Deserialize<'de> for WireConsole {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visit;
        impl<'de> serde::de::Visitor<'de> for Visit {
            type Value = WireConsole;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a console")
            }

            fn visit_enum<A: serde::de::EnumAccess<'de>>(
                self,
                data: A,
            ) -> Result<WireConsole, A::Error> {
                use serde::de::VariantAccess;
                let (index, variant): (u32, _) = data.variant()?;
                if index == NDS_WIRE {
                    let code = variant.newtype_variant()?;
                    return Ok(WireConsole::Known(Console::Nds { code }));
                }
                variant.unit_variant()?;
                Ok(Console::from_wire(index).map_or(WireConsole::Unknown, WireConsole::Known))
            }
        }
        deserializer.deserialize_enum("Console", &[], Visit)
    }
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
    #[serde(skip)]
    pub touch: Option<[u8; 2]>,
}

impl Pad {
    pub const PACED: u16 = 1 << 14;
    pub const EXACT: u16 = 1 << 15;
    pub const BUTTONS: u16 = !(Self::PACED | Self::EXACT);
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
    Download,
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
        cable: Option<Vec<u8>>,
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
    Got {
        game: GameHash,
        upto: u32,
    },
    DsUser {
        round: u32,
        user: DsUser,
    },
    Delay {
        round: u32,
        delay: u8,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DsUser {
    pub nickname: String,
    pub message: String,
    pub color: u8,
    pub birthday: (u8, u8),
    pub language: u8,
    pub mac: [u8; 6],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejoinPoint {
    pub session: SessionId,
    pub round: u32,
    pub frame: u32,
    pub oldest: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GamePart {
    pub hash: GameHash,
    pub offset: u32,
    pub total: u32,
    pub data: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct WirePart {
    hash: GameHash,
    offset: u32,
    total: u32,
    packed: Vec<u8>,
}

pub fn game_cap(sharing: u32, console: Console) -> u32 {
    match console.is_ds() && sharing >= MAX_GAME_BYTES {
        true => MAX_DS_GAME_BYTES,
        false => sharing,
    }
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
                console: match card.console {
                    WireConsole::Known(console) => console,
                    WireConsole::Unknown => return Err(OfferError::UnknownConsole),
                },
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
    fn wire(&self) -> WirePart {
        let sample = &self.data[..self.data.len().min(GAME_SAMPLE_BYTES)];
        let shrinks = pack(sample, true).len() <= sample.len();
        WirePart {
            hash: self.hash,
            offset: self.offset,
            total: self.total,
            packed: pack(&self.data, shrinks),
        }
    }

    pub fn seal(&self, keys: &Keys, to: &PublicKey) -> Sealed {
        self.seal_for(&keys.sealer(to))
    }

    pub fn seal_for(&self, sealer: &PeerSealer) -> Sealed {
        sealer.seal(&postcard::to_allocvec(&self.wire()).expect("game parts always encode"))
    }

    pub fn open(keys: &Keys, from: &PublicKey, sealed: &Sealed) -> Result<Self, OpenError> {
        let wire: WirePart = open(keys, from, sealed)?;
        let data = unpack(&wire.packed, GAME_PART_BYTES).map_err(|_| OpenError)?;
        Ok(Self {
            hash: wire.hash,
            offset: wire.offset,
            total: wire.total,
            data,
        })
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
                        touch: None,
                    },
                    Pad {
                        buttons: 0,
                        cable: Some(false),
                        touch: None,
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
                    touch: None,
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
                cable: None,
            },
            PlayMsg::Resync {
                round: 2,
                frame: 120,
                unit: 3,
                data: vec![6; 1000],
                cable: Some(vec![7; 40]),
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
            PlayMsg::Got {
                game: [5; 20],
                upto: MAX_GAME_BYTES,
            },
            PlayMsg::Delay { round: 3, delay: 9 },
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
        assert_eq!(
            tag(&PlayMsg::Got {
                game: [0; 20],
                upto: 0
            }),
            23
        );
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
        Nds([u8; 4]),
        Next,
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
        assert_eq!(
            open(FutureConsole::Nds(*b"IPKE")),
            Ok(Some(Console::Nds { code: *b"IPKE" }))
        );
        let err = open(FutureConsole::Next).unwrap_err();
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
        for console in [
            Console::Gb,
            Console::Gbc,
            Console::Gba,
            Console::Nds { code: *b"IPKE" },
        ] {
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
            total: MAX_DS_GAME_BYTES,
            data: noise(GAME_PART_BYTES),
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
    fn a_game_part_of_padding_crosses_in_a_few_bytes() {
        let (a, b) = pair();
        let part = GamePart {
            hash: [9; 20],
            offset: 0,
            total: MAX_DS_GAME_BYTES,
            data: vec![0xFF; GAME_PART_BYTES],
        };
        let sealed = part.seal(&a, &b.public_key());
        assert!(sealed.to_bytes().len() < 1024);
        assert_eq!(GamePart::open(&b, &a.public_key(), &sealed), Ok(part));
    }

    #[test]
    fn a_part_that_starts_with_noise_goes_unpacked() {
        let mut data = noise(GAME_SAMPLE_BYTES);
        data.resize(GAME_PART_BYTES, 0);
        let part = GamePart {
            hash: [9; 20],
            offset: 0,
            total: MAX_DS_GAME_BYTES,
            data,
        };
        assert_eq!(part.wire().packed.len(), GAME_PART_BYTES + 1);
    }

    #[test]
    fn a_game_part_that_unpacks_past_a_part_is_refused() {
        let (a, b) = pair();
        let wire = WirePart {
            hash: [9; 20],
            offset: 0,
            total: MAX_GAME_BYTES,
            packed: pack(&vec![0; GAME_PART_BYTES + 1], true),
        };
        let sealed = seal(&wire, &a, &b.public_key());
        assert_eq!(GamePart::open(&b, &a.public_key(), &sealed), Err(OpenError));
    }

    #[test]
    fn ds_games_get_the_bigger_cap_where_the_relay_paces_games() {
        let ds = Console::Nds { code: *b"AMCE" };
        assert_eq!(game_cap(MAX_GAME_BYTES, ds), MAX_DS_GAME_BYTES);
        assert_eq!(game_cap(MAX_GAME_BYTES, Console::Gba), MAX_GAME_BYTES);
        assert_eq!(game_cap(UNPACED_GAME_BYTES, ds), UNPACED_GAME_BYTES);
    }

    #[test]
    #[ignore = "frame times, run in release"]
    fn sealing_four_packed_game_parts_stays_short() {
        let (a, b) = pair();
        let mixed: Vec<u8> = noise(GAME_PART_BYTES)
            .chunks(4096)
            .enumerate()
            .flat_map(|(i, c)| match i % 2 {
                0 => c.to_vec(),
                _ => vec![0; c.len()],
            })
            .collect();
        let ms = |started: std::time::Instant| started.elapsed().as_secs_f64() * 1000.0;
        let mut times = Vec::new();
        for (name, data) in [
            ("noise", noise(GAME_PART_BYTES)),
            ("mixed", mixed),
            ("zeros", vec![0; GAME_PART_BYTES]),
        ] {
            let part = GamePart {
                hash: [9; 20],
                offset: 0,
                total: MAX_DS_GAME_BYTES,
                data,
            };
            let started = std::time::Instant::now();
            let packed: Vec<WirePart> = (0..4).map(|_| part.wire()).collect();
            let packing = ms(started);
            let started = std::time::Instant::now();
            let sealed: Vec<Sealed> = packed
                .iter()
                .map(|w| seal(w, &a, &b.public_key()))
                .collect();
            let sealing = ms(started);
            let started = std::time::Instant::now();
            for s in &sealed {
                GamePart::open(&b, &a.public_key(), s).unwrap();
            }
            let opening = ms(started);
            println!(
                "{name}: packing {packing:.2} ms, sealing {sealing:.2} ms, opening {opening:.2} ms"
            );
            times.push((name, packing, sealing, opening));
        }
        for (name, packing, sealing, opening) in times {
            assert!(sealing < 4.0, "{name} sealing {sealing:.2} ms");
            assert!(opening < 8.0, "{name} opening {opening:.2} ms");
            if name == "noise" {
                assert!(packing < 8.0, "{name} packing {packing:.2} ms");
            }
        }
    }

    fn noise(len: usize) -> Vec<u8> {
        let mut state = 0x9E37_79B9u32;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect()
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
    fn this_build_plays_with_play_version_thirteen_only() {
        assert!(!plays_with(12));
        assert!(plays_with(13));
        assert!(!plays_with(14));
    }
}

use crate::mode::{Mode, ScreenShape};
use crate::pairing::Code;
use crate::proof::{Nonce, Proof};
use crate::touch::ScreenTouch;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub const SURFACE_VERSION: u16 = 2;
pub const MAX_NAME_CHARS: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SurfaceId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Resume(pub [u8; 16]);

impl Resume {
    pub fn random() -> Self {
        let mut bytes = [0; 16];
        let _ = getrandom::getrandom(&mut bytes);
        Self(bytes)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SurfaceKind {
    Window,
    Display,
    Device,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Icon {
    pub width: u16,
    pub height: u16,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameCard {
    pub title: String,
    pub system: String,
    pub icon: Option<Icon>,
    pub screens: Vec<ScreenShape>,
    pub played_secs: u64,
    pub paused: bool,
    pub saved_secs_ago: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    pub screen: u8,
    pub seq: u32,
    pub width: u16,
    pub height: u16,
    pub key: bool,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Bye {
    Removed,
    WrongCode,
    NeedsUpdate,
    HostClosed,
    Declined,
    NoGame,
    NoAnswer,
}

impl Bye {
    pub fn label(self) -> &'static str {
        match self {
            Self::Removed => "The other device removed this one",
            Self::WrongCode => "No device has this code",
            Self::NeedsUpdate => "Update Citrine on both devices",
            Self::HostClosed => "The other device closed Citrine",
            Self::Declined => "The other device said not now",
            Self::NoGame => "No game is running on the other device",
            Self::NoAnswer => "The other device didn't answer",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Pause,
    Resume,
    SaveState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostMsg {
    Welcome {
        id: SurfaceId,
        resume: Resume,
        host: String,
    },
    Game(Option<GameCard>),
    Mode(Mode),
    Frame(Frame),
    Bye(Bye),
    Asking,
    Challenge {
        nonce: Nonce,
        host: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SurfaceMsg {
    Hello {
        version: u16,
        name: String,
        kind: SurfaceKind,
        code: Option<Code>,
        resume: Option<Resume>,
    },
    SetMode(Mode),
    Touch(Option<ScreenTouch>),
    Buttons(u16),
    Ack {
        screen: u8,
        seq: u32,
    },
    Action(Action),
    Leave,
    Proof(Option<Proof>),
}

pub trait Wire: Serialize + DeserializeOwned {
    fn encode(&self) -> Vec<u8> {
        postcard::to_stdvec(self).unwrap_or_default()
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        postcard::from_bytes(bytes).ok()
    }
}

impl Wire for HostMsg {}
impl Wire for SurfaceMsg {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mode::{Preset, View};

    #[test]
    fn every_message_round_trips() {
        let host = [
            HostMsg::Welcome {
                id: SurfaceId(3),
                resume: Resume([7; 16]),
                host: "Clara's PC".into(),
            },
            HostMsg::Game(Some(GameCard {
                title: "Pong-NDS".into(),
                system: "Nintendo DS".into(),
                icon: Some(Icon {
                    width: 1,
                    height: 1,
                    rgba: vec![1, 2, 3, 4],
                }),
                screens: vec![ScreenShape {
                    width: 256,
                    height: 192,
                    touch: true,
                }],
                played_secs: 720,
                paused: false,
                saved_secs_ago: Some(180),
            })),
            HostMsg::Game(None),
            HostMsg::Mode(Mode::Custom(vec![View::Screen(1), View::Controller])),
            HostMsg::Frame(Frame {
                screen: 1,
                seq: 9,
                width: 256,
                height: 192,
                key: true,
                data: vec![0; 40],
            }),
            HostMsg::Bye(Bye::Removed),
            HostMsg::Bye(Bye::Declined),
            HostMsg::Asking,
            HostMsg::Challenge {
                nonce: [3; 16],
                host: 9,
            },
            HostMsg::Bye(Bye::NoAnswer),
        ];
        for msg in host {
            assert_eq!(HostMsg::decode(&msg.encode()), Some(msg));
        }
        let surface = [
            SurfaceMsg::Hello {
                version: SURFACE_VERSION,
                name: "Pixel 8".into(),
                kind: SurfaceKind::Device,
                code: Code::parse("482 913"),
                resume: None,
            },
            SurfaceMsg::SetMode(Mode::Preset(Preset::GameInfo)),
            SurfaceMsg::Touch(Some(ScreenTouch {
                screen: 1,
                x: 255,
                y: 191,
            })),
            SurfaceMsg::Touch(None),
            SurfaceMsg::Buttons(0b1010),
            SurfaceMsg::Ack { screen: 1, seq: 9 },
            SurfaceMsg::Action(Action::SaveState),
            SurfaceMsg::Leave,
            SurfaceMsg::Proof(Some([5; 32])),
            SurfaceMsg::Proof(None),
        ];
        for msg in surface {
            assert_eq!(SurfaceMsg::decode(&msg.encode()), Some(msg));
        }
    }

    #[test]
    fn a_hello_starts_with_its_version_so_any_host_can_refuse_it() {
        let hello = SurfaceMsg::Hello {
            version: 1,
            name: "Old phone".into(),
            kind: SurfaceKind::Device,
            code: None,
            resume: None,
        };
        assert_eq!(hello.encode()[..2], [0, 1]);
        let bye = HostMsg::Bye(Bye::NeedsUpdate).encode();
        assert_eq!(bye, [4, 2]);
    }

    #[test]
    fn junk_does_not_decode() {
        assert_eq!(HostMsg::decode(&[250, 1, 2]), None);
        assert_eq!(SurfaceMsg::decode(&[]), None);
    }

    #[test]
    fn resume_tokens_differ() {
        assert_ne!(Resume::random(), Resume::random());
    }
}

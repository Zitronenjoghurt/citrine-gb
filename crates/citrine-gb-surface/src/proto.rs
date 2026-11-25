use crate::mode::{Mode, ScreenShape};
use crate::touch::ScreenTouch;
use citrine_gb_net::{CheckCommit, CheckNonce};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub const SURFACE_VERSION: u16 = 5;
pub const MAX_NAME_CHARS: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SurfaceId(pub u32);

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

impl Icon {
    pub const MAX_SIDE: u16 = 256;

    pub fn is_whole(&self) -> bool {
        self.width <= Self::MAX_SIDE
            && self.height <= Self::MAX_SIDE
            && self.rgba.len() == usize::from(self.width) * usize::from(self.height) * 4
    }
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
    TooManyAsks,
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
            Self::TooManyAsks => "Too many tries. Wait 10 minutes",
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
    Welcome { id: SurfaceId, host: String },
    Game(Option<GameCard>),
    Mode(Mode),
    Frame(Frame),
    Bye(Bye),
    Asking,
    Opened { reply: Vec<u8>, pake: Vec<u8> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Entry {
    Own,
    Code,
    Resume(SurfaceId),
    Asked(CheckCommit),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub version: u16,
    pub name: String,
    pub kind: SurfaceKind,
    empty_v2_fields: [u8; 2],
    pub entry: Entry,
    pub handshake: Vec<u8>,
    pub pake: Vec<u8>,
}

#[derive(Deserialize)]
enum Front {
    Hello { version: u16, name: String },
}

const PROLOGUE: &[u8] = b"citrine screen link v5";

impl Hello {
    pub fn new(name: String, kind: SurfaceKind, entry: Entry) -> Self {
        Self {
            version: SURFACE_VERSION,
            name,
            kind,
            empty_v2_fields: [0; 2],
            entry,
            handshake: Vec::new(),
            pake: Vec::new(),
        }
    }

    pub fn front(bytes: &[u8]) -> Option<(u16, String)> {
        let Front::Hello { version, name } = postcard::from_bytes(bytes).ok()?;
        Some((version, name))
    }

    pub fn prologue(&self) -> Vec<u8> {
        let mut prologue = PROLOGUE.to_vec();
        let said = (self.version, &self.name, self.kind, self.entry, &self.pake);
        prologue.extend(postcard::to_stdvec(&said).unwrap_or_default());
        prologue
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SurfaceMsg {
    Hello(Hello),
    SetMode(Mode),
    Touch(Option<ScreenTouch>),
    Buttons(u16),
    Ack { screen: u8, seq: u32 },
    Action(Action),
    Leave,
    Ready,
    Check(CheckNonce),
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

    #[test]
    fn an_icon_is_whole_only_when_its_pixels_match_its_size() {
        let icon = |width, height, len| Icon {
            width,
            height,
            rgba: vec![0; len],
        };
        assert!(icon(32, 32, 32 * 32 * 4).is_whole());
        assert!(!icon(32, 32, 7).is_whole());
        assert!(!icon(1024, 1, 1024 * 4).is_whole());
    }
    use crate::mode::{Preset, View};

    #[test]
    fn every_message_round_trips() {
        let host = [
            HostMsg::Welcome {
                id: SurfaceId(3),
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
            HostMsg::Opened {
                reply: vec![3; 48],
                pake: vec![5; 33],
            },
            HostMsg::Bye(Bye::NoAnswer),
            HostMsg::Bye(Bye::TooManyAsks),
        ];
        for msg in host {
            assert_eq!(HostMsg::decode(&msg.encode()), Some(msg));
        }
        let surface = [
            SurfaceMsg::Hello(Hello::new(
                "Pixel 8".into(),
                SurfaceKind::Device,
                Entry::Resume(SurfaceId(4)),
            )),
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
            SurfaceMsg::Ready,
            SurfaceMsg::Check(CheckNonce::from_bytes([6; 32])),
            SurfaceMsg::Hello(Hello::new(
                "Pixel 8".into(),
                SurfaceKind::Device,
                Entry::Asked(CheckNonce::from_bytes([6; 32]).commit()),
            )),
        ];
        for msg in surface {
            assert_eq!(SurfaceMsg::decode(&msg.encode()), Some(msg));
        }
    }

    #[test]
    fn a_hello_starts_with_its_version_so_any_host_can_refuse_it() {
        let hello = SurfaceMsg::Hello(Hello::new(
            "Pixel 8".into(),
            SurfaceKind::Device,
            Entry::Own,
        ));
        assert_eq!(hello.encode()[..2], [0, 5]);
        let bye = HostMsg::Bye(Bye::NeedsUpdate).encode();
        assert_eq!(bye, [4, 2]);
    }

    #[derive(Debug, Deserialize)]
    enum HelloOf0_12 {
        Hello {
            version: u16,
            name: String,
            kind: SurfaceKind,
            code: Option<crate::pairing::Code>,
            resume: Option<[u8; 16]>,
        },
    }

    #[test]
    fn a_0_12_host_reads_the_hello_far_enough_to_refuse_it() {
        let mut hello = Hello::new("Pixel 8".into(), SurfaceKind::Device, Entry::Code);
        hello.handshake = vec![7; 32];
        let bytes = SurfaceMsg::Hello(hello).encode();
        let HelloOf0_12::Hello {
            version,
            name,
            kind,
            code,
            resume,
        } = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(version, 5);
        assert_eq!(name, "Pixel 8");
        assert_eq!((kind, code, resume), (SurfaceKind::Device, None, None));
    }

    #[test]
    fn the_front_of_any_hello_names_its_version_and_device() {
        let commit = CheckNonce::from_bytes([1; 32]).commit();
        let new = SurfaceMsg::Hello(Hello::new(
            "Mac".into(),
            SurfaceKind::Window,
            Entry::Asked(commit),
        ));
        assert_eq!(Hello::front(&new.encode()), Some((5, "Mac".into())));
        let of_0_12 = [0, 2, 7, b'P', b'i', b'x', b'e', b'l', b' ', b'8', 2, 0, 0];
        assert_eq!(Hello::front(&of_0_12), Some((2, "Pixel 8".into())));
        assert_eq!(Hello::front(&SurfaceMsg::Ready.encode()), None);
    }

    #[test]
    fn the_prologue_holds_all_the_hello_says_but_the_handshake() {
        let hello = Hello::new("Pixel 8".into(), SurfaceKind::Device, Entry::Own);
        let mut renamed = hello.clone();
        renamed.name = "Pixel 9".into();
        let mut asked = hello.clone();
        asked.entry = Entry::Asked(CheckNonce::from_bytes([1; 32]).commit());
        let mut recommitted = hello.clone();
        recommitted.entry = Entry::Asked(CheckNonce::from_bytes([2; 32]).commit());
        let mut shaken = hello.clone();
        shaken.handshake = vec![1; 32];
        let mut paked = hello.clone();
        paked.pake = vec![2; 33];
        assert_ne!(hello.prologue(), renamed.prologue());
        assert_ne!(hello.prologue(), asked.prologue());
        assert_ne!(asked.prologue(), recommitted.prologue());
        assert_ne!(hello.prologue(), paked.prologue());
        assert_eq!(hello.prologue(), shaken.prologue());
    }

    #[test]
    fn junk_does_not_decode() {
        assert_eq!(HostMsg::decode(&[250, 1, 2]), None);
        assert_eq!(SurfaceMsg::decode(&[]), None);
    }
}

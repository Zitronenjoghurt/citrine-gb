use crate::crypto::{AuthKey, PublicKey, Sealed};
use crate::id::OnlineId;
use crate::invite::InviteCode;
use crate::saves::{GameKey, SaveInfo};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fmt;

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_FRAME_BYTES: usize = 1 << 20;
pub const CHUNK_BYTES: usize = 256 * 1024;
pub const MAX_SCREEN_BYTES: usize = 64 * 1024;

pub type GameHash = [u8; 20];
pub type SessionId = u64;
pub type MessageId = u64;
pub type Tag = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FileId(pub [u8; 16]);

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayPassword(pub String);

impl fmt::Debug for RelayPassword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RelayPassword(..)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
    pub protocol: u16,
    pub password_required: bool,
    pub sign_ups_open: bool,
    pub invites_required: bool,
    pub quota_bytes: u64,
    pub max_message_bytes: u64,
    pub max_file_bytes: u64,
    pub game_sharing: bool,
    pub pictures: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub online_id: OnlineId,
    pub display_name: String,
    pub storage_used: u64,
    pub quota_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FriendState {
    Friend,
    Incoming,
    Outgoing,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Presence {
    Online { away: bool },
    Offline { last_seen: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Friend {
    pub online_id: OnlineId,
    pub display_name: String,
    pub public_key: PublicKey,
    pub state: FriendState,
    pub presence: Option<Presence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: MessageId,
    pub from: OnlineId,
    pub at: u64,
    pub sealed: Sealed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileInfo {
    pub id: FileId,
    pub size: u64,
    pub created: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EndReason {
    Declined,
    Left,
    Disconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Notice {
    RelayOff,
    KillSwitch,
    Disconnected,
    Banned,
    PasswordChanged,
    SignedInElsewhere,
    ShuttingDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum ErrorCode {
    #[error("the server speaks protocol {server}")]
    Protocol { server: u16 },
    #[error("wrong relay password")]
    WrongPassword,
    #[error("this server takes no new accounts")]
    SignUpsClosed,
    #[error("this server needs an invite code for new accounts")]
    InviteNeeded,
    #[error("the invite code is wrong")]
    InviteWrong,
    #[error("the invite code was already used")]
    InviteUsed,
    #[error("no account with this relay ID")]
    UnknownAccount,
    #[error("an account with this relay ID exists")]
    AccountExists,
    #[error("this account is banned")]
    Banned,
    #[error("sign in first")]
    NotSignedIn,
    #[error("already signed in")]
    AlreadySignedIn,
    #[error("the display name isn't allowed")]
    BadName,
    #[error("no one has this online ID")]
    UnknownUser,
    #[error("you aren't friends")]
    NotFriends,
    #[error("your friend is offline")]
    Offline,
    #[error("too large")]
    TooLarge,
    #[error("your storage is full")]
    QuotaFull,
    #[error("this server doesn't store files")]
    SharingOff,
    #[error("the file doesn't exist")]
    UnknownFile,
    #[error("the session doesn't exist")]
    UnknownSession,
    #[error("this server doesn't pass on game files")]
    GameSharingOff,
    #[error("sending too fast")]
    RateLimited,
    #[error("too many tries, wait a minute")]
    TooManyTries,
    #[error("the server couldn't read the message")]
    BadMessage,
    #[error("the server had a problem")]
    Server,
    #[error("the save changed on another device")]
    SaveChanged,
    #[error("this server doesn't allow Discord screenshots")]
    PicturesOff,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientMsg {
    Hello {
        protocol: u16,
        app: String,
    },
    SignUp {
        relay_password: Option<RelayPassword>,
        auth_key: AuthKey,
        public_key: PublicKey,
        display_name: String,
        invite: Option<InviteCode>,
    },
    SignIn {
        relay_password: Option<RelayPassword>,
        auth_key: AuthKey,
    },
    SetDisplayName(String),
    SetStatus {
        away: bool,
    },
    AddFriend(OnlineId),
    AnswerFriend {
        from: OnlineId,
        accept: bool,
    },
    RemoveFriend(OnlineId),
    Block(OnlineId),
    Unblock(OnlineId),
    Chat {
        tag: Tag,
        to: OnlineId,
        sealed: Sealed,
    },
    ChatAck(Vec<MessageId>),
    Upload {
        tag: Tag,
        size: u64,
    },
    UploadChunk {
        tag: Tag,
        data: Vec<u8>,
    },
    Download(FileId),
    DeleteFile(FileId),
    ListFiles,
    Invite {
        to: OnlineId,
        sealed: Sealed,
    },
    AnswerInvite {
        session: SessionId,
        accept: bool,
    },
    Leave(SessionId),
    Play {
        session: SessionId,
        sealed: Sealed,
    },
    Game {
        session: SessionId,
        sealed: Sealed,
    },
    Ping(u64),
    Playing {
        to: OnlineId,
        sealed: Sealed,
    },
    ListSaves,
    PutSave {
        tag: Tag,
        game: GameKey,
        slot: String,
        base: u64,
        label: Vec<u8>,
        data: Vec<u8>,
    },
    GetSave {
        game: GameKey,
        slot: String,
    },
    DeleteSave {
        tag: Tag,
        game: GameKey,
        slot: String,
        base: u64,
    },
    Screen(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerMsg {
    Info(ServerInfo),
    Welcome(Account),
    Account(Account),
    Friends(Vec<Friend>),
    Friend(Friend),
    FriendGone(OnlineId),
    Presence {
        who: OnlineId,
        presence: Presence,
    },
    ChatSent {
        tag: Tag,
        id: MessageId,
        at: u64,
    },
    ChatFailed {
        tag: Tag,
        error: ErrorCode,
    },
    Chat(ChatMessage),
    UploadReady(Tag),
    Uploaded {
        tag: Tag,
        file: FileInfo,
    },
    UploadFailed {
        tag: Tag,
        error: ErrorCode,
    },
    FileStart {
        file: FileId,
        size: u64,
    },
    FileChunk {
        file: FileId,
        data: Vec<u8>,
    },
    DownloadFailed {
        file: FileId,
        error: ErrorCode,
    },
    FileDeleted(FileId),
    Files(Vec<FileInfo>),
    Invited {
        session: SessionId,
        to: OnlineId,
    },
    Invite {
        session: SessionId,
        from: OnlineId,
        sealed: Sealed,
    },
    SessionStarted {
        session: SessionId,
        with: OnlineId,
    },
    SessionEnded {
        session: SessionId,
        reason: EndReason,
    },
    Play {
        session: SessionId,
        sealed: Sealed,
    },
    Game {
        session: SessionId,
        sealed: Sealed,
    },
    Pong(u64),
    Error(ErrorCode),
    Notice(Notice),
    Playing {
        from: OnlineId,
        sealed: Sealed,
    },
    Saves(Vec<SaveInfo>),
    SaveStored {
        tag: Tag,
        info: SaveInfo,
    },
    SaveRefused {
        tag: Tag,
        error: ErrorCode,
        current: Option<SaveInfo>,
    },
    Save {
        info: SaveInfo,
        data: Vec<u8>,
    },
    SaveMissing {
        game: GameKey,
        slot: String,
    },
    SaveDeleted {
        tag: Tag,
    },
    ScreenStored {
        path: String,
    },
    ScreenRefused(ErrorCode),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CodecError {
    #[error("the message is {0} bytes, over the limit")]
    TooLarge(usize),
    #[error("the message is malformed")]
    Malformed,
}

fn encode<T: Serialize>(msg: &T) -> Result<Vec<u8>, CodecError> {
    let bytes = postcard::to_allocvec(msg).map_err(|_| CodecError::Malformed)?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(CodecError::TooLarge(bytes.len()));
    }
    Ok(bytes)
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, CodecError> {
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(CodecError::TooLarge(bytes.len()));
    }
    match postcard::take_from_bytes(bytes) {
        Ok((msg, [])) => Ok(msg),
        _ => Err(CodecError::Malformed),
    }
}

impl ClientMsg {
    pub fn encode(&self) -> Result<Vec<u8>, CodecError> {
        encode(self)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        decode(bytes)
    }
}

impl ServerMsg {
    pub fn encode(&self) -> Result<Vec<u8>, CodecError> {
        encode(self)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        decode(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RelayId;
    use std::collections::BTreeSet;

    fn friend_id() -> OnlineId {
        OnlineId::new(0x12_3456_789A).unwrap()
    }

    fn sealed() -> Sealed {
        let alice = RelayId::from_bytes([1; 32]).keys();
        let bob = RelayId::from_bytes([2; 32]).keys();
        alice.seal(&bob.public_key(), "hi 🍋".as_bytes())
    }

    fn save_info() -> SaveInfo {
        SaveInfo {
            game: GameKey([7; 32]),
            slot: "battery".into(),
            version: 4,
            size: 300,
            updated: 1_700_000_000,
            label: vec![1; 40],
        }
    }

    fn file() -> FileId {
        FileId([0xAB; 16])
    }

    fn client_name(msg: &ClientMsg) -> &'static str {
        match msg {
            ClientMsg::Hello { .. } => "Hello",
            ClientMsg::SignUp { .. } => "SignUp",
            ClientMsg::SignIn { .. } => "SignIn",
            ClientMsg::SetDisplayName(_) => "SetDisplayName",
            ClientMsg::SetStatus { .. } => "SetStatus",
            ClientMsg::AddFriend(_) => "AddFriend",
            ClientMsg::AnswerFriend { .. } => "AnswerFriend",
            ClientMsg::RemoveFriend(_) => "RemoveFriend",
            ClientMsg::Block(_) => "Block",
            ClientMsg::Unblock(_) => "Unblock",
            ClientMsg::Chat { .. } => "Chat",
            ClientMsg::ChatAck(_) => "ChatAck",
            ClientMsg::Upload { .. } => "Upload",
            ClientMsg::UploadChunk { .. } => "UploadChunk",
            ClientMsg::Download(_) => "Download",
            ClientMsg::DeleteFile(_) => "DeleteFile",
            ClientMsg::ListFiles => "ListFiles",
            ClientMsg::Invite { .. } => "Invite",
            ClientMsg::AnswerInvite { .. } => "AnswerInvite",
            ClientMsg::Leave(_) => "Leave",
            ClientMsg::Play { .. } => "Play",
            ClientMsg::Game { .. } => "Game",
            ClientMsg::Ping(_) => "Ping",
            ClientMsg::Playing { .. } => "Playing",
            ClientMsg::ListSaves => "ListSaves",
            ClientMsg::PutSave { .. } => "PutSave",
            ClientMsg::GetSave { .. } => "GetSave",
            ClientMsg::DeleteSave { .. } => "DeleteSave",
            ClientMsg::Screen(_) => "Screen",
        }
    }

    fn server_name(msg: &ServerMsg) -> &'static str {
        match msg {
            ServerMsg::Info(_) => "Info",
            ServerMsg::Welcome(_) => "Welcome",
            ServerMsg::Account(_) => "Account",
            ServerMsg::Friends(_) => "Friends",
            ServerMsg::Friend(_) => "Friend",
            ServerMsg::FriendGone(_) => "FriendGone",
            ServerMsg::Presence { .. } => "Presence",
            ServerMsg::ChatSent { .. } => "ChatSent",
            ServerMsg::ChatFailed { .. } => "ChatFailed",
            ServerMsg::Chat(_) => "Chat",
            ServerMsg::UploadReady(_) => "UploadReady",
            ServerMsg::Uploaded { .. } => "Uploaded",
            ServerMsg::UploadFailed { .. } => "UploadFailed",
            ServerMsg::FileStart { .. } => "FileStart",
            ServerMsg::FileChunk { .. } => "FileChunk",
            ServerMsg::DownloadFailed { .. } => "DownloadFailed",
            ServerMsg::FileDeleted(_) => "FileDeleted",
            ServerMsg::Files(_) => "Files",
            ServerMsg::Invited { .. } => "Invited",
            ServerMsg::Invite { .. } => "Invite",
            ServerMsg::SessionStarted { .. } => "SessionStarted",
            ServerMsg::SessionEnded { .. } => "SessionEnded",
            ServerMsg::Play { .. } => "Play",
            ServerMsg::Game { .. } => "Game",
            ServerMsg::Pong(_) => "Pong",
            ServerMsg::Error(_) => "Error",
            ServerMsg::Notice(_) => "Notice",
            ServerMsg::Playing { .. } => "Playing",
            ServerMsg::Saves(_) => "Saves",
            ServerMsg::SaveStored { .. } => "SaveStored",
            ServerMsg::SaveRefused { .. } => "SaveRefused",
            ServerMsg::Save { .. } => "Save",
            ServerMsg::SaveMissing { .. } => "SaveMissing",
            ServerMsg::SaveDeleted { .. } => "SaveDeleted",
            ServerMsg::ScreenStored { .. } => "ScreenStored",
            ServerMsg::ScreenRefused(_) => "ScreenRefused",
        }
    }

    fn all_client() -> Vec<ClientMsg> {
        let keys = RelayId::from_bytes([3; 32]).keys();
        vec![
            ClientMsg::Hello {
                protocol: PROTOCOL_VERSION,
                app: "0.8.0 web".into(),
            },
            ClientMsg::SignUp {
                relay_password: Some(RelayPassword("hunter2".into())),
                auth_key: keys.auth_key().clone(),
                public_key: keys.public_key(),
                display_name: "Clara".into(),
                invite: Some("7KQ2-M9XD-4HTB".parse().unwrap()),
            },
            ClientMsg::SignIn {
                relay_password: None,
                auth_key: keys.auth_key().clone(),
            },
            ClientMsg::SetDisplayName("Zitrone".into()),
            ClientMsg::SetStatus { away: true },
            ClientMsg::AddFriend(friend_id()),
            ClientMsg::AnswerFriend {
                from: friend_id(),
                accept: false,
            },
            ClientMsg::RemoveFriend(friend_id()),
            ClientMsg::Block(friend_id()),
            ClientMsg::Unblock(friend_id()),
            ClientMsg::Chat {
                tag: 9,
                to: friend_id(),
                sealed: sealed(),
            },
            ClientMsg::ChatAck(vec![1, 2, u64::MAX]),
            ClientMsg::Upload {
                tag: 1,
                size: 30_000,
            },
            ClientMsg::UploadChunk {
                tag: 1,
                data: vec![0x55; 300],
            },
            ClientMsg::Download(file()),
            ClientMsg::DeleteFile(file()),
            ClientMsg::ListFiles,
            ClientMsg::Invite {
                to: friend_id(),
                sealed: sealed(),
            },
            ClientMsg::AnswerInvite {
                session: 77,
                accept: true,
            },
            ClientMsg::Leave(77),
            ClientMsg::Play {
                session: 77,
                sealed: sealed(),
            },
            ClientMsg::Game {
                session: 77,
                sealed: sealed(),
            },
            ClientMsg::Ping(123),
            ClientMsg::Playing {
                to: friend_id(),
                sealed: sealed(),
            },
            ClientMsg::ListSaves,
            ClientMsg::PutSave {
                tag: 5,
                game: GameKey([7; 32]),
                slot: "battery".into(),
                base: 3,
                label: vec![1; 40],
                data: vec![2; 300],
            },
            ClientMsg::GetSave {
                game: GameKey([7; 32]),
                slot: "slot-2".into(),
            },
            ClientMsg::DeleteSave {
                tag: 6,
                game: GameKey([7; 32]),
                slot: "slot-2".into(),
                base: 4,
            },
            ClientMsg::Screen(vec![4; 500]),
        ]
    }

    fn all_server() -> Vec<ServerMsg> {
        let account = Account {
            online_id: friend_id(),
            display_name: "Clara".into(),
            storage_used: 1234,
            quota_bytes: 100 << 20,
        };
        let friend = Friend {
            online_id: friend_id(),
            display_name: "Ana".into(),
            public_key: PublicKey([4; 32]),
            state: FriendState::Friend,
            presence: Some(Presence::Offline { last_seen: 99 }),
        };
        let info = FileInfo {
            id: file(),
            size: 10,
            created: 5,
        };
        vec![
            ServerMsg::Info(ServerInfo {
                name: "Lemon".into(),
                version: "0.8.0".into(),
                protocol: PROTOCOL_VERSION,
                password_required: true,
                sign_ups_open: false,
                invites_required: true,
                quota_bytes: 100 << 20,
                max_message_bytes: 64 << 10,
                max_file_bytes: 25 << 20,
                game_sharing: true,
                pictures: true,
            }),
            ServerMsg::Welcome(account.clone()),
            ServerMsg::Account(account),
            ServerMsg::Friends(vec![
                friend.clone(),
                Friend {
                    state: FriendState::Incoming,
                    presence: None,
                    ..friend.clone()
                },
            ]),
            ServerMsg::Friend(Friend {
                presence: Some(Presence::Online { away: false }),
                ..friend
            }),
            ServerMsg::FriendGone(friend_id()),
            ServerMsg::Presence {
                who: friend_id(),
                presence: Presence::Online { away: true },
            },
            ServerMsg::ChatSent {
                tag: 9,
                id: 4,
                at: 1_700_000_000,
            },
            ServerMsg::ChatFailed {
                tag: 9,
                error: ErrorCode::NotFriends,
            },
            ServerMsg::Chat(ChatMessage {
                id: 4,
                from: friend_id(),
                at: 1_700_000_000,
                sealed: sealed(),
            }),
            ServerMsg::UploadReady(1),
            ServerMsg::Uploaded { tag: 1, file: info },
            ServerMsg::UploadFailed {
                tag: 1,
                error: ErrorCode::QuotaFull,
            },
            ServerMsg::FileStart {
                file: file(),
                size: 10,
            },
            ServerMsg::FileChunk {
                file: file(),
                data: vec![9; 10],
            },
            ServerMsg::DownloadFailed {
                file: file(),
                error: ErrorCode::UnknownFile,
            },
            ServerMsg::FileDeleted(file()),
            ServerMsg::Files(vec![info]),
            ServerMsg::Invited {
                session: 77,
                to: friend_id(),
            },
            ServerMsg::Invite {
                session: 77,
                from: friend_id(),
                sealed: sealed(),
            },
            ServerMsg::SessionStarted {
                session: 77,
                with: friend_id(),
            },
            ServerMsg::SessionEnded {
                session: 77,
                reason: EndReason::Disconnected,
            },
            ServerMsg::Play {
                session: 77,
                sealed: sealed(),
            },
            ServerMsg::Game {
                session: 77,
                sealed: sealed(),
            },
            ServerMsg::Pong(123),
            ServerMsg::Error(ErrorCode::Protocol { server: 2 }),
            ServerMsg::Notice(Notice::RelayOff),
            ServerMsg::Playing {
                from: friend_id(),
                sealed: sealed(),
            },
            ServerMsg::Saves(vec![save_info()]),
            ServerMsg::SaveStored {
                tag: 5,
                info: save_info(),
            },
            ServerMsg::SaveRefused {
                tag: 5,
                error: ErrorCode::SaveChanged,
                current: Some(save_info()),
            },
            ServerMsg::Save {
                info: save_info(),
                data: vec![3; 200],
            },
            ServerMsg::SaveMissing {
                game: GameKey([7; 32]),
                slot: "slot-2".into(),
            },
            ServerMsg::SaveDeleted { tag: 6 },
            ServerMsg::ScreenStored {
                path: "/f/0123456789abcdef0123456789abcdef".into(),
            },
            ServerMsg::ScreenRefused(ErrorCode::RateLimited),
        ]
    }

    #[test]
    fn every_client_message_round_trips() {
        let all = all_client();
        let names: BTreeSet<_> = all.iter().map(client_name).collect();
        assert_eq!(names.len(), 29);
        assert_eq!(names.len(), all.len());
        for msg in all {
            let bytes = msg.encode().unwrap();
            assert_eq!(ClientMsg::decode(&bytes), Ok(msg));
        }
    }

    #[test]
    fn every_server_message_round_trips() {
        let all = all_server();
        let names: BTreeSet<_> = all.iter().map(server_name).collect();
        assert_eq!(names.len(), 36);
        assert_eq!(names.len(), all.len());
        for msg in all {
            let bytes = msg.encode().unwrap();
            assert_eq!(ServerMsg::decode(&bytes), Ok(msg));
        }
    }

    #[test]
    fn every_error_and_notice_round_trips() {
        let errors = [
            ErrorCode::Protocol { server: 1 },
            ErrorCode::WrongPassword,
            ErrorCode::SignUpsClosed,
            ErrorCode::InviteNeeded,
            ErrorCode::InviteWrong,
            ErrorCode::InviteUsed,
            ErrorCode::UnknownAccount,
            ErrorCode::AccountExists,
            ErrorCode::Banned,
            ErrorCode::NotSignedIn,
            ErrorCode::AlreadySignedIn,
            ErrorCode::BadName,
            ErrorCode::UnknownUser,
            ErrorCode::NotFriends,
            ErrorCode::Offline,
            ErrorCode::TooLarge,
            ErrorCode::QuotaFull,
            ErrorCode::SharingOff,
            ErrorCode::UnknownFile,
            ErrorCode::UnknownSession,
            ErrorCode::GameSharingOff,
            ErrorCode::RateLimited,
            ErrorCode::TooManyTries,
            ErrorCode::BadMessage,
            ErrorCode::Server,
            ErrorCode::SaveChanged,
            ErrorCode::PicturesOff,
        ];
        for error in errors {
            let msg = ServerMsg::Error(error);
            assert_eq!(ServerMsg::decode(&msg.encode().unwrap()), Ok(msg));
        }
        let notices = [
            Notice::RelayOff,
            Notice::KillSwitch,
            Notice::Disconnected,
            Notice::Banned,
            Notice::PasswordChanged,
            Notice::SignedInElsewhere,
            Notice::ShuttingDown,
        ];
        for notice in notices {
            let msg = ServerMsg::Notice(notice);
            assert_eq!(ServerMsg::decode(&msg.encode().unwrap()), Ok(msg));
        }
        for reason in [
            EndReason::Declined,
            EndReason::Left,
            EndReason::Disconnected,
        ] {
            let msg = ServerMsg::SessionEnded { session: 1, reason };
            assert_eq!(ServerMsg::decode(&msg.encode().unwrap()), Ok(msg));
        }
    }

    #[test]
    fn play_and_ping_messages_stay_small() {
        let play = ClientMsg::Play {
            session: u64::MAX,
            sealed: Sealed {
                nonce: [0xFF; crate::NONCE_BYTES],
                ciphertext: vec![0xFF; crate::SEAL_OVERHEAD + 8],
            },
        };
        assert!(play.encode().unwrap().len() <= 64);
        assert!(ClientMsg::Ping(1).encode().unwrap().len() <= 3);
    }

    #[test]
    fn a_full_chunk_fits_in_a_frame() {
        let chunk = ClientMsg::UploadChunk {
            tag: Tag::MAX,
            data: vec![0xFF; CHUNK_BYTES],
        };
        assert!(chunk.encode().unwrap().len() < MAX_FRAME_BYTES);
        let chunk = ServerMsg::FileChunk {
            file: file(),
            data: vec![0xFF; CHUNK_BYTES],
        };
        assert!(chunk.encode().unwrap().len() < MAX_FRAME_BYTES);
    }

    #[test]
    fn encode_refuses_over_the_cap() {
        let msg = ClientMsg::UploadChunk {
            tag: 1,
            data: vec![0; MAX_FRAME_BYTES],
        };
        assert!(matches!(msg.encode(), Err(CodecError::TooLarge(_))));
    }

    #[test]
    fn decode_refuses_over_the_cap_before_parsing() {
        let bytes = vec![0; MAX_FRAME_BYTES + 1];
        assert_eq!(
            ClientMsg::decode(&bytes),
            Err(CodecError::TooLarge(MAX_FRAME_BYTES + 1))
        );
        assert_eq!(
            ServerMsg::decode(&bytes),
            Err(CodecError::TooLarge(MAX_FRAME_BYTES + 1))
        );
    }

    #[test]
    fn decode_refuses_garbage_truncation_and_trailing_bytes() {
        assert_eq!(ClientMsg::decode(&[]), Err(CodecError::Malformed));
        assert_eq!(ClientMsg::decode(&[0xFF, 0xFF]), Err(CodecError::Malformed));
        let mut bytes = ClientMsg::Ping(500).encode().unwrap();
        bytes.push(0);
        assert_eq!(ClientMsg::decode(&bytes), Err(CodecError::Malformed));
        let bytes = ClientMsg::SetDisplayName("Clara".into()).encode().unwrap();
        assert_eq!(
            ClientMsg::decode(&bytes[..bytes.len() - 1]),
            Err(CodecError::Malformed)
        );
        let huge_len = [3, 0xFF, 0xFF, 0xFF, 0xFF, 0x0F];
        assert_eq!(ClientMsg::decode(&huge_len), Err(CodecError::Malformed));
    }

    #[test]
    fn debug_never_shows_the_relay_password() {
        let msg = ClientMsg::SignIn {
            relay_password: Some(RelayPassword("hunter2".into())),
            auth_key: RelayId::from_bytes([3; 32]).keys().auth_key().clone(),
        };
        assert!(!format!("{msg:?}").contains("hunter2"));
    }
}

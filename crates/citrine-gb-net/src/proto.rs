use crate::archive::{ArchiveGame, ArchiveId, ArchiveKey};
use crate::crypto::{AuthKey, DeviceKey, PublicKey, Sealed};
use crate::id::OnlineId;
use crate::invite::InviteCode;
use crate::saves::{GameKey, SaveInfo};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fmt;

pub const PROTOCOL_VERSION: u16 = 11;
pub const OLDEST_PROTOCOL_VERSION: u16 = 2;
pub const SAVE_PARTS_PROTOCOL: u16 = 4;
pub const ROOMS_PROTOCOL: u16 = 4;
pub const GAME_BUCKET_PROTOCOL: u16 = 5;
pub const PUSH_PROTOCOL: u16 = 6;
pub const GTS_PROTOCOL: u16 = 7;
pub const DS_NET_PROTOCOL: u16 = 8;
pub const DIRECT_PROTOCOL: u16 = 10;
pub const MAX_DATAGRAM_BYTES: usize = 2048;
pub const MAX_NET_READ_BYTES: usize = 16 * 1024;
pub const NET_WINDOW_BYTES: usize = 64 * 1024;
pub const GTS_PAGE: usize = 20;
pub const MAX_GTS_TARGET_CHARS: usize = 4096;
pub const MAX_DS_HOST_KEYS: usize = 32;
pub const MAX_DS_HOST_TEXT: usize = 64;
pub const MAX_DS_HOSTS_LISTED: usize = 64;
pub const MAX_DS_MESSAGE_BYTES: usize = 256;
pub const MAX_PUSH_TOKEN_BYTES: usize = 4096;
pub const ROOM_MEMBERS: usize = 16;
pub const MAX_FRAME_BYTES: usize = 1 << 20;
pub const CHUNK_BYTES: usize = 256 * 1024;
pub const MAX_SCREEN_BYTES: usize = 64 * 1024;

pub type GameHash = [u8; 20];
pub type SessionId = u64;
pub type MessageId = u64;
pub type Tag = u32;
pub type DeviceId = u64;
pub const MAX_DEVICE_NAME_CHARS: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FileId(pub [u8; 16]);

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayPassword(pub String);

impl fmt::Debug for RelayPassword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RelayPassword(..)")
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OneTimeCode(pub String);

impl fmt::Debug for OneTimeCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OneTimeCode(..)")
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
    pub two_factor: bool,
    pub codes_left: u8,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OldChat {
    pub friend: OnlineId,
    pub key: u64,
    pub at: u64,
    pub sealed: Sealed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileInfo {
    pub id: FileId,
    pub size: u64,
    pub created: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: DeviceId,
    pub name: String,
    pub added: u64,
    pub last_seen: u64,
    pub online: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MemberState {
    pub shows_screen: bool,
    pub playing: bool,
    pub reconnecting: bool,
    pub round_trip_ms: Option<u16>,
    pub old_client: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub who: OnlineId,
    pub display_name: String,
    pub public_key: PublicKey,
    pub state: MemberState,
}

pub fn speaks_protocol(version: u16) -> bool {
    (OLDEST_PROTOCOL_VERSION..=PROTOCOL_VERSION).contains(&version)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EndReason {
    Declined,
    Left,
    Disconnected,
    NoCopy,
    OtherCopy,
    NoAnswer,
}

impl EndReason {
    pub fn for_protocol(self, protocol: u16) -> Self {
        match self {
            Self::NoCopy | Self::OtherCopy | Self::NoAnswer if protocol < PUSH_PROTOCOL => {
                Self::Declined
            }
            reason => reason,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Refusal {
    NoCopy,
    OtherCopy,
}

impl Refusal {
    pub fn reason(self) -> EndReason {
        match self {
            Self::NoCopy => EndReason::NoCopy,
            Self::OtherCopy => EndReason::OtherCopy,
        }
    }
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
    SignedOut,
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
    #[error("their inbox is full")]
    InboxFull,
    #[error("this account asks for a two-factor code")]
    CodeNeeded,
    #[error("the two-factor code is wrong")]
    CodeWrong,
    #[error("another device signed this one out")]
    SignedOut,
    #[error("no such device")]
    UnknownDevice,
    #[error("the room is full")]
    RoomFull,
    #[error("they can't join this room")]
    CantJoin,
    #[error("someone in the room needs a newer app")]
    NeedsUpdate,
    #[error("this relay has no GTS")]
    GtsOff,
    #[error("this relay doesn't carry DS online play")]
    DsNetOff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NetAddr {
    pub ip: [u8; 4],
    pub port: u16,
}

impl NetAddr {
    pub fn socket(self) -> std::net::SocketAddrV4 {
        std::net::SocketAddrV4::new(self.ip.into(), self.port)
    }
}

impl From<std::net::SocketAddrV4> for NetAddr {
    fn from(addr: std::net::SocketAddrV4) -> Self {
        Self {
            ip: addr.ip().octets(),
            port: addr.port(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DsHost {
    pub addr: NetAddr,
    pub keys: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GtsReply {
    Body(#[serde(with = "serde_bytes")] Vec<u8>),
    Status(u16),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GtsPokemon {
    pub species: u16,
    pub level: u8,
    pub gender: u8,
    pub shiny: bool,
    pub nickname: String,
    pub language: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GtsWanted {
    pub species: u16,
    pub gender: u8,
    pub min_level: u8,
    pub max_level: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GtsListing {
    pub offered: GtsPokemon,
    pub wanted: GtsWanted,
    pub trainer: String,
    pub game: u8,
    pub country: u8,
    pub deposited: u64,
    pub friend: Option<OnlineId>,
    pub simulated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GtsState {
    Waiting,
    Traded {
        at: u64,
        received: GtsPokemon,
        trainer: String,
        simulated: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GtsDeposit {
    pub generation: u8,
    pub listing: GtsListing,
    pub state: GtsState,
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
        device_name: String,
    },
    SignIn {
        relay_password: Option<RelayPassword>,
        auth_key: AuthKey,
        device: Option<DeviceKey>,
        code: Option<OneTimeCode>,
        device_name: String,
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
        #[serde(with = "serde_bytes")]
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
        #[serde(with = "serde_bytes")]
        label: Vec<u8>,
        #[serde(with = "serde_bytes")]
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
    Screen(#[serde(with = "serde_bytes")] Vec<u8>),
    TwoFactorStart,
    TwoFactorConfirm(OneTimeCode),
    TwoFactorOff(OneTimeCode),
    NewRecoveryCodes(OneTimeCode),
    DeleteAccount {
        code: Option<OneTimeCode>,
    },
    SignOutDevice(DeviceId),
    Restate {
        id: MessageId,
        sealed: Option<Sealed>,
    },
    Typing {
        to: OnlineId,
        typing: bool,
    },
    RoomInvite {
        room: SessionId,
        to: OnlineId,
        sealed: Sealed,
    },
    MemberUpdate {
        room: SessionId,
        shows_screen: bool,
        playing: bool,
    },
    RoomPlay {
        room: SessionId,
        to: OnlineId,
        sealed: Sealed,
    },
    RoomGame {
        room: SessionId,
        to: OnlineId,
        sealed: Sealed,
    },
    RoomChat {
        room: SessionId,
        copies: Vec<(OnlineId, Sealed)>,
    },
    ChatOnce {
        tag: Tag,
        to: OnlineId,
        key: u64,
        sealed: Sealed,
    },
    KeepOldChats {
        chats: Vec<OldChat>,
        done: bool,
    },
    SavePart {
        tag: Tag,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
    },
    SetPush {
        token: Option<String>,
        kinds: PushKinds,
    },
    Refuse {
        session: SessionId,
        why: Refusal,
    },
    Gts {
        call: u64,
        target: String,
    },
    GtsBrowse {
        generation: u8,
        species: Option<u16>,
        page: u32,
    },
    GtsMine,
    NetSend {
        port: u16,
        to: NetAddr,
        data: Vec<u8>,
    },
    NetOpen {
        stream: u32,
        to: NetAddr,
    },
    NetWrite {
        stream: u32,
        data: Vec<u8>,
    },
    NetClose {
        stream: u32,
    },
    NetLease {
        want: Option<[u8; 4]>,
    },
    NetTaken {
        stream: u32,
        total: u64,
    },
    DsProfile {
        call: u64,
        profile: u32,
    },
    DsHost {
        game: String,
        port: u16,
        keys: Vec<(String, String)>,
    },
    DsUnhost,
    DsList {
        call: u64,
        game: String,
    },
    DsMessage {
        to: NetAddr,
        data: Vec<u8>,
    },
    DsNatneg {
        cookie: u32,
        index: u8,
        port: u16,
    },
    ArchiveGet {
        id: ArchiveId,
        from_part: u32,
    },
    ArchiveCancel(ArchiveId),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PushKinds {
    pub friend_requests: bool,
    pub invites: bool,
    pub chats: bool,
}

impl PushKinds {
    pub const ALL: Self = Self {
        friend_requests: true,
        invites: true,
        chats: true,
    };

    pub fn bits(self) -> u8 {
        u8::from(self.friend_requests) | u8::from(self.invites) << 1 | u8::from(self.chats) << 2
    }

    pub fn from_bits(bits: u8) -> Self {
        Self {
            friend_requests: bits & 1 != 0,
            invites: bits & 2 != 0,
            chats: bits & 4 != 0,
        }
    }
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
        #[serde(with = "serde_bytes")]
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
        #[serde(with = "serde_bytes")]
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
    Device(DeviceKey),
    TwoFactorSetup {
        secret: Vec<u8>,
    },
    RecoveryCodes(Vec<String>),
    AccountDeleted,
    Devices {
        list: Vec<DeviceInfo>,
        this: DeviceId,
    },
    ChatCopy {
        to: OnlineId,
        message: ChatMessage,
    },
    SavesChanged,
    Typing {
        from: OnlineId,
        typing: bool,
    },
    RoomJoined {
        room: SessionId,
        members: Vec<Member>,
    },
    MemberUpdate {
        room: SessionId,
        member: Member,
    },
    MemberGone {
        room: SessionId,
        who: OnlineId,
        reason: EndReason,
    },
    RoomPlay {
        room: SessionId,
        from: OnlineId,
        sealed: Sealed,
    },
    RoomGame {
        room: SessionId,
        from: OnlineId,
        sealed: Sealed,
    },
    RoomChat {
        room: SessionId,
        from: OnlineId,
        at: u64,
        sealed: Sealed,
    },
    OldChatsWanted,
    OldChatsKept,
    OldChat {
        id: MessageId,
        chat: OldChat,
    },
    SavePart {
        game: GameKey,
        slot: String,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
    },
    Gts {
        call: u64,
        reply: GtsReply,
    },
    GtsListings {
        generation: u8,
        species: Option<u16>,
        page: u32,
        total: u32,
        listings: Vec<GtsListing>,
        simulated: bool,
    },
    GtsMine(Vec<GtsDeposit>),
    GtsTraded(GtsDeposit),
    GtsSimulated,
    GtsTradedWithSim {
        trainer: String,
        received: GtsPokemon,
    },
    NetHeard {
        port: u16,
        from: NetAddr,
        data: Vec<u8>,
    },
    NetOpened {
        stream: u32,
        ok: bool,
    },
    NetRead {
        stream: u32,
        data: Vec<u8>,
    },
    NetClosed {
        stream: u32,
    },
    NetLeased {
        ip: Option<[u8; 4]>,
    },
    NetWritten {
        stream: u32,
        total: u64,
    },
    Stun {
        port: u16,
    },
    DsProfile {
        call: u64,
        ours: bool,
    },
    DsList {
        call: u64,
        hosts: Vec<DsHost>,
    },
    DsMessage {
        from: [u8; 4],
        port: u16,
        data: Vec<u8>,
    },
    DsNatneg {
        cookie: u32,
        peer: NetAddr,
    },
    Archive(Vec<ArchiveGame>),
    ArchiveStart {
        id: ArchiveId,
        key: ArchiveKey,
        prefix: [u8; 16],
        parts: u32,
    },
    ArchivePart {
        id: ArchiveId,
        index: u32,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
    },
    ArchiveFailed {
        id: ArchiveId,
        error: ErrorCode,
    },
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
            ClientMsg::Refuse { .. } => "Refuse",
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
            ClientMsg::TwoFactorStart => "TwoFactorStart",
            ClientMsg::TwoFactorConfirm(_) => "TwoFactorConfirm",
            ClientMsg::TwoFactorOff(_) => "TwoFactorOff",
            ClientMsg::NewRecoveryCodes(_) => "NewRecoveryCodes",
            ClientMsg::DeleteAccount { .. } => "DeleteAccount",
            ClientMsg::SignOutDevice(_) => "SignOutDevice",
            ClientMsg::Restate { .. } => "Restate",
            ClientMsg::Typing { .. } => "Typing",
            ClientMsg::RoomInvite { .. } => "RoomInvite",
            ClientMsg::MemberUpdate { .. } => "MemberUpdate",
            ClientMsg::RoomPlay { .. } => "RoomPlay",
            ClientMsg::RoomGame { .. } => "RoomGame",
            ClientMsg::RoomChat { .. } => "RoomChat",
            ClientMsg::ChatOnce { .. } => "ChatOnce",
            ClientMsg::KeepOldChats { .. } => "KeepOldChats",
            ClientMsg::SavePart { .. } => "SavePart",
            ClientMsg::SetPush { .. } => "SetPush",
            ClientMsg::Gts { .. } => "Gts",
            ClientMsg::GtsBrowse { .. } => "GtsBrowse",
            ClientMsg::GtsMine => "GtsMine",
            ClientMsg::NetSend { .. } => "NetSend",
            ClientMsg::NetOpen { .. } => "NetOpen",
            ClientMsg::NetWrite { .. } => "NetWrite",
            ClientMsg::NetClose { .. } => "NetClose",
            ClientMsg::NetLease { .. } => "NetLease",
            ClientMsg::NetTaken { .. } => "NetTaken",
            ClientMsg::DsProfile { .. } => "DsProfile",
            ClientMsg::DsHost { .. } => "DsHost",
            ClientMsg::DsUnhost => "DsUnhost",
            ClientMsg::DsList { .. } => "DsList",
            ClientMsg::DsMessage { .. } => "DsMessage",
            ClientMsg::DsNatneg { .. } => "DsNatneg",
            ClientMsg::ArchiveGet { .. } => "ArchiveGet",
            ClientMsg::ArchiveCancel(_) => "ArchiveCancel",
        }
    }

    fn server_name(msg: &ServerMsg) -> &'static str {
        match msg {
            ServerMsg::Stun { .. } => "Stun",
            ServerMsg::DsProfile { .. } => "DsProfile",
            ServerMsg::DsList { .. } => "DsList",
            ServerMsg::DsMessage { .. } => "DsMessage",
            ServerMsg::DsNatneg { .. } => "DsNatneg",
            ServerMsg::Archive(_) => "Archive",
            ServerMsg::ArchiveStart { .. } => "ArchiveStart",
            ServerMsg::ArchivePart { .. } => "ArchivePart",
            ServerMsg::ArchiveFailed { .. } => "ArchiveFailed",
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
            ServerMsg::Device(_) => "Device",
            ServerMsg::TwoFactorSetup { .. } => "TwoFactorSetup",
            ServerMsg::RecoveryCodes(_) => "RecoveryCodes",
            ServerMsg::AccountDeleted => "AccountDeleted",
            ServerMsg::Devices { .. } => "Devices",
            ServerMsg::ChatCopy { .. } => "ChatCopy",
            ServerMsg::SavesChanged => "SavesChanged",
            ServerMsg::Typing { .. } => "Typing",
            ServerMsg::RoomJoined { .. } => "RoomJoined",
            ServerMsg::MemberUpdate { .. } => "MemberUpdate",
            ServerMsg::MemberGone { .. } => "MemberGone",
            ServerMsg::RoomPlay { .. } => "RoomPlay",
            ServerMsg::RoomGame { .. } => "RoomGame",
            ServerMsg::RoomChat { .. } => "RoomChat",
            ServerMsg::OldChatsWanted => "OldChatsWanted",
            ServerMsg::OldChatsKept => "OldChatsKept",
            ServerMsg::OldChat { .. } => "OldChat",
            ServerMsg::SavePart { .. } => "SavePart",
            ServerMsg::Gts { .. } => "Gts",
            ServerMsg::GtsListings { .. } => "GtsListings",
            ServerMsg::GtsMine(_) => "GtsMine",
            ServerMsg::GtsTraded(_) => "GtsTraded",
            ServerMsg::GtsSimulated => "GtsSimulated",
            ServerMsg::GtsTradedWithSim { .. } => "GtsTradedWithSim",
            ServerMsg::NetHeard { .. } => "NetHeard",
            ServerMsg::NetOpened { .. } => "NetOpened",
            ServerMsg::NetRead { .. } => "NetRead",
            ServerMsg::NetClosed { .. } => "NetClosed",
            ServerMsg::NetLeased { .. } => "NetLeased",
            ServerMsg::NetWritten { .. } => "NetWritten",
        }
    }

    fn member() -> Member {
        Member {
            who: friend_id(),
            display_name: "Jonas".into(),
            public_key: RelayId::from_bytes([5; 32]).keys().public_key(),
            state: MemberState {
                shows_screen: false,
                playing: true,
                reconnecting: false,
                round_trip_ms: Some(48),
                old_client: false,
            },
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
                device_name: "Firefox on Mac".into(),
            },
            ClientMsg::SignIn {
                relay_password: None,
                auth_key: keys.auth_key().clone(),
                device: Some(DeviceKey([6; 32])),
                code: Some(OneTimeCode("123456".into())),
                device_name: "Android phone".into(),
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
            ClientMsg::TwoFactorStart,
            ClientMsg::TwoFactorConfirm(OneTimeCode("123456".into())),
            ClientMsg::TwoFactorOff(OneTimeCode("1234-5678-9012".into())),
            ClientMsg::NewRecoveryCodes(OneTimeCode("654321".into())),
            ClientMsg::DeleteAccount { code: None },
            ClientMsg::SignOutDevice(4),
            ClientMsg::Restate {
                id: 9,
                sealed: Some(sealed()),
            },
            ClientMsg::Typing {
                to: friend_id(),
                typing: false,
            },
            ClientMsg::RoomInvite {
                room: 7,
                to: friend_id(),
                sealed: sealed(),
            },
            ClientMsg::MemberUpdate {
                room: 7,
                shows_screen: true,
                playing: true,
            },
            ClientMsg::RoomPlay {
                room: 7,
                to: friend_id(),
                sealed: sealed(),
            },
            ClientMsg::RoomGame {
                room: 7,
                to: friend_id(),
                sealed: sealed(),
            },
            ClientMsg::RoomChat {
                room: 7,
                copies: vec![(friend_id(), sealed()), (friend_id(), sealed())],
            },
            ClientMsg::ChatOnce {
                tag: 9,
                to: friend_id(),
                key: u64::MAX,
                sealed: sealed(),
            },
            ClientMsg::KeepOldChats {
                chats: vec![OldChat {
                    friend: friend_id(),
                    key: 7,
                    at: 1_700_000_000,
                    sealed: sealed(),
                }],
                done: true,
            },
            ClientMsg::SavePart {
                tag: 4,
                data: vec![0x66; 300],
            },
            ClientMsg::SetPush {
                token: Some("fcm".into()),
                kinds: PushKinds::ALL,
            },
            ClientMsg::Refuse {
                session: 77,
                why: Refusal::OtherCopy,
            },
            ClientMsg::Gts {
                call: 3,
                target: "/pokemondpds/worldexchange/info.asp?pid=1".into(),
            },
            ClientMsg::GtsBrowse {
                generation: 4,
                species: Some(396),
                page: 2,
            },
            ClientMsg::GtsMine,
            ClientMsg::NetSend {
                port: 5000,
                to: NetAddr {
                    ip: [203, 0, 113, 9],
                    port: 27900,
                },
                data: vec![1; 170],
            },
            ClientMsg::NetOpen {
                stream: 3,
                to: NetAddr {
                    ip: [203, 0, 113, 9],
                    port: 29900,
                },
            },
            ClientMsg::NetWrite {
                stream: 3,
                data: b"\\login\\".to_vec(),
            },
            ClientMsg::NetClose { stream: 3 },
            ClientMsg::NetLease {
                want: Some([10, 77, 1, 2]),
            },
            ClientMsg::NetTaken {
                stream: 3,
                total: 5,
            },
            ClientMsg::DsProfile {
                call: 4,
                profile: 0x1234_5678,
            },
            ClientMsg::DsHost {
                game: "mariokartds".into(),
                port: 56400,
                keys: vec![("dwc_pid".into(), "603819417".into())],
            },
            ClientMsg::DsUnhost,
            ClientMsg::DsList {
                call: 5,
                game: "mariokartds".into(),
            },
            ClientMsg::DsMessage {
                to: NetAddr {
                    ip: [10, 77, 1, 2],
                    port: 56400,
                },
                data: vec![0xFD, 0xFC, 1, 2],
            },
            ClientMsg::DsNatneg {
                cookie: 0x0102_0304,
                index: 1,
                port: 56400,
            },
            ClientMsg::ArchiveGet {
                id: ArchiveId([3; 16]),
                from_part: 7,
            },
            ClientMsg::ArchiveCancel(ArchiveId([3; 16])),
        ]
    }

    fn all_server() -> Vec<ServerMsg> {
        let account = Account {
            online_id: friend_id(),
            display_name: "Clara".into(),
            storage_used: 1234,
            quota_bytes: 100 << 20,
            two_factor: true,
            codes_left: 9,
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
            ServerMsg::Device(DeviceKey([8; 32])),
            ServerMsg::TwoFactorSetup {
                secret: vec![1; 20],
            },
            ServerMsg::RecoveryCodes(vec!["1234-5678-9012".into()]),
            ServerMsg::AccountDeleted,
            ServerMsg::Devices {
                list: vec![DeviceInfo {
                    id: 3,
                    name: "Android phone".into(),
                    added: 1_700_000_000,
                    last_seen: 1_700_000_100,
                    online: true,
                }],
                this: 3,
            },
            ServerMsg::ChatCopy {
                to: friend_id(),
                message: ChatMessage {
                    id: 5,
                    from: friend_id(),
                    at: 1_700_000_000,
                    sealed: sealed(),
                },
            },
            ServerMsg::SavesChanged,
            ServerMsg::Typing {
                from: friend_id(),
                typing: true,
            },
            ServerMsg::RoomJoined {
                room: 7,
                members: vec![member(), member()],
            },
            ServerMsg::MemberUpdate {
                room: 7,
                member: Member {
                    state: MemberState {
                        shows_screen: true,
                        playing: false,
                        reconnecting: true,
                        round_trip_ms: None,
                        old_client: true,
                    },
                    ..member()
                },
            },
            ServerMsg::MemberGone {
                room: 7,
                who: friend_id(),
                reason: EndReason::Disconnected,
            },
            ServerMsg::RoomPlay {
                room: 7,
                from: friend_id(),
                sealed: sealed(),
            },
            ServerMsg::RoomGame {
                room: 7,
                from: friend_id(),
                sealed: sealed(),
            },
            ServerMsg::RoomChat {
                room: 7,
                from: friend_id(),
                at: 1_700_000_000,
                sealed: sealed(),
            },
            ServerMsg::OldChatsWanted,
            ServerMsg::OldChatsKept,
            ServerMsg::OldChat {
                id: 6,
                chat: OldChat {
                    friend: friend_id(),
                    key: 7,
                    at: 1_700_000_000,
                    sealed: sealed(),
                },
            },
            ServerMsg::SavePart {
                game: GameKey([3; 32]),
                slot: "slot0".into(),
                data: vec![0x77; 300],
            },
            ServerMsg::Gts {
                call: 3,
                reply: GtsReply::Body(vec![1, 0]),
            },
            ServerMsg::GtsListings {
                generation: 4,
                species: None,
                page: 0,
                total: 1,
                listings: vec![gts_listing()],
                simulated: true,
            },
            ServerMsg::GtsMine(vec![GtsDeposit {
                generation: 4,
                listing: gts_listing(),
                state: GtsState::Waiting,
            }]),
            ServerMsg::GtsTraded(GtsDeposit {
                generation: 4,
                listing: gts_listing(),
                state: GtsState::Traded {
                    at: 9,
                    received: gts_listing().offered,
                    trainer: "Ren".into(),
                    simulated: true,
                },
            }),
            ServerMsg::GtsSimulated,
            ServerMsg::GtsTradedWithSim {
                trainer: "Ren".into(),
                received: gts_listing().offered,
            },
            ServerMsg::NetHeard {
                port: 5000,
                from: NetAddr {
                    ip: [203, 0, 113, 9],
                    port: 27900,
                },
                data: vec![2; 170],
            },
            ServerMsg::NetOpened {
                stream: 3,
                ok: true,
            },
            ServerMsg::NetRead {
                stream: 3,
                data: b"\\lc\\1".to_vec(),
            },
            ServerMsg::NetClosed { stream: 3 },
            ServerMsg::NetLeased {
                ip: Some([10, 77, 1, 2]),
            },
            ServerMsg::NetWritten {
                stream: 3,
                total: 7,
            },
            ServerMsg::Stun { port: 3478 },
            ServerMsg::DsProfile {
                call: 4,
                ours: false,
            },
            ServerMsg::DsList {
                call: 5,
                hosts: vec![DsHost {
                    addr: NetAddr {
                        ip: [10, 77, 1, 2],
                        port: 56400,
                    },
                    keys: vec![("numplayers".into(), "1".into())],
                }],
            },
            ServerMsg::DsMessage {
                from: [10, 77, 1, 3],
                port: 56400,
                data: vec![0xFD, 0xFC, 1, 2],
            },
            ServerMsg::DsNatneg {
                cookie: 0x0102_0304,
                peer: NetAddr {
                    ip: [10, 77, 1, 3],
                    port: 50000,
                },
            },
            ServerMsg::Archive(vec![ArchiveGame {
                id: ArchiveId([3; 16]),
                name: "Pokemon Platinum".into(),
                console: crate::ArchiveConsole::Ds,
                size: 128 << 20,
                download: 25 << 20,
                sha256: [1; 32],
                sha1: [2; 20],
                crc32: 0xDEAD_BEEF,
            }]),
            ServerMsg::ArchiveStart {
                id: ArchiveId([3; 16]),
                key: ArchiveKey([4; 32]),
                prefix: [5; 16],
                parts: 100,
            },
            ServerMsg::ArchivePart {
                id: ArchiveId([3; 16]),
                index: 99,
                data: vec![6; 40],
            },
            ServerMsg::ArchiveFailed {
                id: ArchiveId([3; 16]),
                error: ErrorCode::UnknownFile,
            },
        ]
    }

    fn gts_listing() -> GtsListing {
        GtsListing {
            offered: GtsPokemon {
                species: 396,
                level: 5,
                gender: 1,
                shiny: false,
                nickname: "STARLY".into(),
                language: 2,
            },
            wanted: GtsWanted {
                species: 25,
                gender: 0,
                min_level: 1,
                max_level: 100,
            },
            trainer: "Lucas".into(),
            game: 12,
            country: 0,
            deposited: 1_700_000_000,
            friend: Some(friend_id()),
            simulated: false,
        }
    }

    #[test]
    fn every_client_message_round_trips() {
        let all = all_client();
        let names: BTreeSet<_> = all.iter().map(client_name).collect();
        assert_eq!(names.len(), 64);
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
        assert_eq!(names.len(), 75);
        assert_eq!(names.len(), all.len());
        for msg in all {
            let bytes = msg.encode().unwrap();
            assert_eq!(ServerMsg::decode(&bytes), Ok(msg));
        }
    }

    #[test]
    fn a_restate_that_deletes_round_trips() {
        let msg = ClientMsg::Restate {
            id: 9,
            sealed: None,
        };
        assert_eq!(ClientMsg::decode(&msg.encode().unwrap()), Ok(msg));
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
            ErrorCode::InboxFull,
            ErrorCode::CodeNeeded,
            ErrorCode::CodeWrong,
            ErrorCode::SignedOut,
            ErrorCode::UnknownDevice,
            ErrorCode::RoomFull,
            ErrorCode::CantJoin,
            ErrorCode::NeedsUpdate,
            ErrorCode::GtsOff,
            ErrorCode::DsNetOff,
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
            Notice::SignedOut,
        ];
        for notice in notices {
            let msg = ServerMsg::Notice(notice);
            assert_eq!(ServerMsg::decode(&msg.encode().unwrap()), Ok(msg));
        }
        for reason in [
            EndReason::Declined,
            EndReason::Left,
            EndReason::Disconnected,
            EndReason::NoCopy,
            EndReason::OtherCopy,
            EndReason::NoAnswer,
        ] {
            let msg = ServerMsg::SessionEnded { session: 1, reason };
            assert_eq!(ServerMsg::decode(&msg.encode().unwrap()), Ok(msg));
        }
    }

    #[test]
    fn older_apps_hear_new_end_reasons_as_declined() {
        for reason in [EndReason::NoCopy, EndReason::OtherCopy, EndReason::NoAnswer] {
            assert_eq!(reason.for_protocol(PUSH_PROTOCOL - 1), EndReason::Declined);
            assert_eq!(reason.for_protocol(PUSH_PROTOCOL), reason);
        }
        assert_eq!(EndReason::Left.for_protocol(2), EndReason::Left);
        assert_eq!(Refusal::NoCopy.reason(), EndReason::NoCopy);
        assert_eq!(Refusal::OtherCopy.reason(), EndReason::OtherCopy);
    }

    #[test]
    fn room_messages_take_new_tags_after_the_old_ones() {
        let all = all_client();
        let tags: Vec<u8> = all.iter().map(|m| m.encode().unwrap()[0]).collect();
        assert_eq!(tags, (0..64).collect::<Vec<u8>>());
        assert_eq!(client_name(&all[10]), "Chat");
        assert_eq!(client_name(&all[45]), "SetPush");
        assert_eq!(client_name(&all[46]), "Refuse");
        assert_eq!(client_name(&all[35]), "Restate");
        assert_eq!(client_name(&all[36]), "Typing");
        assert_eq!(client_name(&all[42]), "ChatOnce");
        assert_eq!(client_name(&all[47]), "Gts");
        assert_eq!(client_name(&all[49]), "GtsMine");
        assert_eq!(client_name(&all[53]), "NetClose");
        assert_eq!(client_name(&all[54]), "NetLease");
        assert_eq!(client_name(&all[55]), "NetTaken");
        assert_eq!(client_name(&all[56]), "DsProfile");
        assert_eq!(client_name(&all[57]), "DsHost");
        assert_eq!(client_name(&all[58]), "DsUnhost");
        assert_eq!(client_name(&all[59]), "DsList");
        assert_eq!(client_name(&all[60]), "DsMessage");
        assert_eq!(client_name(&all[61]), "DsNatneg");
        assert_eq!(client_name(&all[62]), "ArchiveGet");
        assert_eq!(client_name(&all[63]), "ArchiveCancel");
        let all = all_server();
        let tags: Vec<u8> = all.iter().map(|m| m.encode().unwrap()[0]).collect();
        assert_eq!(tags, (0..75).collect::<Vec<u8>>());
        assert_eq!(server_name(&all[42]), "SavesChanged");
        assert_eq!(server_name(&all[43]), "Typing");
        assert_eq!(server_name(&all[54]), "Gts");
        assert_eq!(server_name(&all[57]), "GtsTraded");
        assert_eq!(server_name(&all[59]), "GtsTradedWithSim");
        assert_eq!(server_name(&all[63]), "NetClosed");
        assert_eq!(server_name(&all[64]), "NetLeased");
        assert_eq!(server_name(&all[65]), "NetWritten");
        assert_eq!(server_name(&all[66]), "Stun");
        assert_eq!(server_name(&all[67]), "DsProfile");
        assert_eq!(server_name(&all[68]), "DsList");
        assert_eq!(server_name(&all[69]), "DsMessage");
        assert_eq!(server_name(&all[70]), "DsNatneg");
        assert_eq!(server_name(&all[71]), "Archive");
        assert_eq!(server_name(&all[74]), "ArchiveFailed");
    }

    #[test]
    fn a_lease_ask_and_a_refusal_round_trip() {
        let ask = ClientMsg::NetLease { want: None };
        assert_eq!(ClientMsg::decode(&ask.encode().unwrap()), Ok(ask));
        let refusal = ServerMsg::NetLeased { ip: None };
        assert_eq!(ServerMsg::decode(&refusal.encode().unwrap()), Ok(refusal));
    }

    #[test]
    fn this_build_speaks_protocol_two_to_eleven() {
        assert!(!speaks_protocol(1));
        for version in 2..=11 {
            assert!(speaks_protocol(version), "protocol {version}");
        }
        assert!(!speaks_protocol(12));
    }

    #[test]
    fn a_room_pad_batch_stays_small() {
        let msg = ClientMsg::RoomPlay {
            room: u64::MAX,
            to: friend_id(),
            sealed: Sealed {
                nonce: [0xFF; crate::NONCE_BYTES],
                ciphertext: vec![0xFF; crate::SEAL_OVERHEAD + 9],
            },
        };
        assert!(msg.encode().unwrap().len() <= 76);
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
        let part = ClientMsg::SavePart {
            tag: Tag::MAX,
            data: vec![0xFF; crate::SAVE_PART_BYTES],
        };
        assert!(part.encode().unwrap().len() < MAX_FRAME_BYTES);
        let part = ServerMsg::SavePart {
            game: GameKey([0xFF; 32]),
            slot: "x".repeat(crate::MAX_SLOT_CHARS),
            data: vec![0xFF; crate::SAVE_PART_BYTES],
        };
        assert!(part.encode().unwrap().len() < MAX_FRAME_BYTES);
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
            device: Some(DeviceKey([0xEE; 32])),
            code: Some(OneTimeCode("481220".into())),
            device_name: "Linux PC".into(),
        };
        let text = format!("{msg:?}");
        assert!(!text.contains("hunter2"));
        assert!(!text.contains("481220"));
        assert!(!text.contains("238"));
    }

    mod old {
        use super::*;

        #[derive(Serialize, Deserialize)]
        pub struct Info {
            pub game: GameKey,
            pub slot: String,
            pub version: u64,
            pub size: u64,
            pub updated: u64,
            pub label: Vec<u8>,
        }

        #[derive(Serialize, Deserialize)]
        pub struct PutSave {
            pub tag: Tag,
            pub game: GameKey,
            pub slot: String,
            pub base: u64,
            pub label: Vec<u8>,
            pub data: Vec<u8>,
        }

        #[derive(Serialize, Deserialize)]
        pub struct ClientPart {
            pub tag: Tag,
            pub data: Vec<u8>,
        }

        #[derive(Serialize, Deserialize)]
        pub struct Save {
            pub info: Info,
            pub data: Vec<u8>,
        }

        #[derive(Serialize, Deserialize)]
        pub struct ServerPart {
            pub game: GameKey,
            pub slot: String,
            pub data: Vec<u8>,
        }

        #[derive(Serialize, Deserialize)]
        pub struct UploadChunk {
            pub tag: Tag,
            pub data: Vec<u8>,
        }

        #[derive(Serialize, Deserialize)]
        pub struct FileChunk {
            pub file: FileId,
            pub data: Vec<u8>,
        }

        #[derive(Serialize, Deserialize)]
        pub enum GtsReply {
            Body(Vec<u8>),
            Status(u16),
        }

        #[derive(Serialize, Deserialize)]
        pub struct Gts {
            pub call: u64,
            pub reply: GtsReply,
        }

        #[derive(Serialize, Deserialize)]
        pub struct Sealed {
            pub nonce: [u8; crate::NONCE_BYTES],
            pub ciphertext: Vec<u8>,
        }
    }

    fn pattern(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 7 + 3) as u8).collect()
    }

    fn old_frame(variant: u8, body: &impl Serialize) -> Vec<u8> {
        let mut bytes = vec![variant];
        bytes.extend(postcard::to_allocvec(body).unwrap());
        bytes
    }

    #[test]
    fn save_bytes_keep_their_exact_wire_bytes() {
        let part = ClientMsg::SavePart {
            tag: 0x102,
            data: vec![0xAB, 0xCD, 0xEF],
        };
        assert_eq!(
            part.encode().unwrap(),
            [44, 0x82, 0x02, 3, 0xAB, 0xCD, 0xEF]
        );
        let part = ServerMsg::SavePart {
            game: GameKey([9; 32]),
            slot: "a".into(),
            data: pattern(300),
        };
        let mut want = vec![53];
        want.extend([9; 32]);
        want.extend([1, b'a', 0xAC, 0x02]);
        want.extend(pattern(300));
        assert_eq!(part.encode().unwrap(), want);
    }

    #[test]
    fn save_messages_match_and_read_the_old_vec_encoding() {
        let info = SaveInfo {
            game: GameKey([4; 32]),
            slot: "slot-1".into(),
            version: 300,
            size: 1 << 20,
            updated: 1_700_000_000,
            label: pattern(200),
        };
        let old_info = || old::Info {
            game: info.game,
            slot: info.slot.clone(),
            version: info.version,
            size: info.size,
            updated: info.updated,
            label: info.label.clone(),
        };
        let data = pattern(crate::SAVE_PART_BYTES);
        let client = [
            (
                ClientMsg::PutSave {
                    tag: 7,
                    game: info.game,
                    slot: info.slot.clone(),
                    base: 3,
                    label: pattern(150),
                    data: data.clone(),
                },
                old_frame(
                    25,
                    &old::PutSave {
                        tag: 7,
                        game: info.game,
                        slot: info.slot.clone(),
                        base: 3,
                        label: pattern(150),
                        data: data.clone(),
                    },
                ),
            ),
            (
                ClientMsg::SavePart {
                    tag: Tag::MAX,
                    data: data.clone(),
                },
                old_frame(
                    44,
                    &old::ClientPart {
                        tag: Tag::MAX,
                        data: data.clone(),
                    },
                ),
            ),
        ];
        for (msg, old) in client {
            assert_eq!(msg.encode().unwrap(), old);
            assert_eq!(ClientMsg::decode(&old), Ok(msg));
        }
        let server = [
            (
                ServerMsg::Save {
                    info: info.clone(),
                    data: data.clone(),
                },
                old_frame(
                    31,
                    &old::Save {
                        info: old_info(),
                        data: data.clone(),
                    },
                ),
            ),
            (
                ServerMsg::SavePart {
                    game: info.game,
                    slot: info.slot.clone(),
                    data: data.clone(),
                },
                old_frame(
                    53,
                    &old::ServerPart {
                        game: info.game,
                        slot: info.slot.clone(),
                        data: data.clone(),
                    },
                ),
            ),
            (
                ServerMsg::Saves(vec![info.clone(), info.clone()]),
                old_frame(28, &vec![old_info(), old_info()]),
            ),
        ];
        for (msg, old) in server {
            assert_eq!(msg.encode().unwrap(), old);
            assert_eq!(ServerMsg::decode(&old), Ok(msg));
        }
    }

    #[test]
    fn file_screen_gts_and_sealed_bytes_match_the_old_vec_encoding() {
        assert_eq!(
            ClientMsg::UploadChunk {
                tag: 0x102,
                data: vec![0xAB, 0xCD, 0xEF],
            }
            .encode()
            .unwrap(),
            [13, 0x82, 0x02, 3, 0xAB, 0xCD, 0xEF]
        );
        assert_eq!(
            ClientMsg::Screen(vec![1, 2]).encode().unwrap(),
            [28, 2, 1, 2]
        );
        let data = pattern(CHUNK_BYTES);
        let screen = pattern(MAX_SCREEN_BYTES);
        let client = [
            (
                ClientMsg::UploadChunk {
                    tag: Tag::MAX,
                    data: data.clone(),
                },
                old_frame(
                    13,
                    &old::UploadChunk {
                        tag: Tag::MAX,
                        data: data.clone(),
                    },
                ),
            ),
            (ClientMsg::Screen(screen.clone()), old_frame(28, &screen)),
        ];
        for (msg, old) in client {
            assert_eq!(msg.encode().unwrap(), old);
            assert_eq!(ClientMsg::decode(&old), Ok(msg));
        }
        let file = FileId([5; 16]);
        let server = [
            (
                ServerMsg::FileChunk {
                    file,
                    data: data.clone(),
                },
                old_frame(
                    14,
                    &old::FileChunk {
                        file,
                        data: data.clone(),
                    },
                ),
            ),
            (
                ServerMsg::Gts {
                    call: 300,
                    reply: GtsReply::Body(pattern(400)),
                },
                old_frame(
                    54,
                    &old::Gts {
                        call: 300,
                        reply: old::GtsReply::Body(pattern(400)),
                    },
                ),
            ),
            (
                ServerMsg::Gts {
                    call: 1,
                    reply: GtsReply::Status(404),
                },
                old_frame(
                    54,
                    &old::Gts {
                        call: 1,
                        reply: old::GtsReply::Status(404),
                    },
                ),
            ),
        ];
        for (msg, old) in server {
            assert_eq!(msg.encode().unwrap(), old);
            assert_eq!(ServerMsg::decode(&old), Ok(msg));
        }
        let sealed = Sealed {
            nonce: [3; crate::NONCE_BYTES],
            ciphertext: pattern(300),
        };
        let old_sealed = postcard::to_allocvec(&old::Sealed {
            nonce: sealed.nonce,
            ciphertext: sealed.ciphertext.clone(),
        })
        .unwrap();
        let mut want = vec![3; crate::NONCE_BYTES];
        want.extend([0xAC, 0x02]);
        want.extend(pattern(300));
        assert_eq!(old_sealed, want);
        assert_eq!(postcard::to_allocvec(&sealed).unwrap(), want);
        assert_eq!(postcard::from_bytes::<Sealed>(&want), Ok(sealed));
    }
}

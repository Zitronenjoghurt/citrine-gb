mod chat;
mod crypto;
mod id;
mod invite;
mod name;
mod play;
mod proto;
mod saves;
mod secure;

pub use chat::{
    Body, ChatError, Edit, Envelope, MAX_PALETTE_BYTES, MAX_RECEIPT_IDS, MAX_TEXT_CHARS,
    MAX_UNPACKED_BYTES, Receipt, Share, ShareKind, pack, unpack,
};
pub use crypto::{
    AuthKey, DeviceKey, Keys, NONCE_BYTES, OpenError, PublicKey, SEAL_OVERHEAD, Sealed,
};
pub use id::{IdError, OnlineId, RelayId};
pub use invite::InviteCode;
pub use name::{
    DISPLAY_NAME_MAX_CHARS, NameError, avatar_hues, device_name, display_name, initials, is_phone,
};
pub use play::*;
pub use proto::*;
pub use saves::{
    GameKey, MAX_LABEL_BYTES, MAX_SAVE_BYTES, MAX_SLOT_CHARS, STORAGE_BLOCK, SaveInfo, SaveLabel,
    SavePayload, storage_cost, valid_slot,
};
pub use secure::{
    Channel, ClientHandshake, Fingerprint, MAGIC, Opener, RelayKey, RelaySecret, Sealer,
    SecureError, accept, is_secure_start,
};

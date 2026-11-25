pub type GbResult<T> = Result<T, GbError>;

#[derive(Debug, thiserror::Error)]
pub enum GbError {
    #[cfg(feature = "base64")]
    #[error("Base64 decode error: {0}")]
    Base64Decode(#[from] base64::DecodeError),
    #[error("Citrine can't play Game Boy Advance games")]
    GbaGame,
    #[error("Citrine can't play Nintendo DS games")]
    NdsGame,
    #[error("Nintendo DS games can't link or play online yet")]
    NdsLink,
    #[cfg(feature = "nds")]
    #[error("This isn't a Nintendo DS game")]
    NotNds,
    #[cfg(feature = "gba")]
    #[error("This snapshot needs a newer version of Citrine")]
    NewerState,
    #[cfg(feature = "gba")]
    #[error("This snapshot is from an older version of Citrine")]
    OlderState,
    #[error("This snapshot is damaged")]
    DamagedState,
    #[error("This snapshot belongs to another game")]
    OtherGame,
    #[cfg(feature = "gba")]
    #[error("This isn't a Game Boy Advance game")]
    NotGba,
    #[error("IO error: {0}")]
    IO(#[from] std::io::Error),
    #[error("Missing ROM cartridge type")]
    MissingRomCartridgeType,
    #[cfg(feature = "rmp-serde")]
    #[error("RMP decode error: {0}")]
    RmpDecode(#[from] rmp_serde::decode::Error),
    #[cfg(feature = "rmp-serde")]
    #[error("RMP encode error: {0}")]
    RmpEncode(#[from] rmp_serde::encode::Error),
    #[error("ROM too small")]
    RomTooSmall,
    #[error("ROM size exceeded expected rom bank count")]
    RomTooBig,
    #[cfg(feature = "serde_json")]
    #[error("JSON error: {0}")]
    JsonSerde(#[from] serde_json::error::Error),
}

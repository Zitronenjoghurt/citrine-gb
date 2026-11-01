pub type GbResult<T> = Result<T, GbError>;

#[derive(Debug, thiserror::Error)]
pub enum GbError {
    #[cfg(feature = "base64")]
    #[error("Base64 decode error: {0}")]
    Base64Decode(#[from] base64::DecodeError),
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

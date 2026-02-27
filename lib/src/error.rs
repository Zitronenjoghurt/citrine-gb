pub type GbResult<T> = Result<T, GbError>;

#[derive(Debug, thiserror::Error)]
pub enum GbError {
    #[error("IO error: {0}")]
    IO(#[from] std::io::Error),
    #[error("Missing ROM cartridge type")]
    MissingRomCartridgeType,
    #[error("ROM too small")]
    RomTooSmall,
    #[error("ROM size exceeded expected rom bank count")]
    RomTooBig,
    #[cfg(feature = "serde_json")]
    #[error("JSON error: {0}")]
    JsonSerde(#[from] serde_json::error::Error),
}

pub type GbResult<T> = Result<T, GbError>;

#[derive(Debug, thiserror::Error)]
pub enum GbError {
    #[error("IO error: {0}")]
    IO(#[from] std::io::Error),
    #[error("ROM too small")]
    RomTooSmall,
    #[error("ROM size exceeded expected rom bank count")]
    RomTooBig,
}

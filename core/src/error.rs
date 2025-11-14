use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Error: {0}")]
    Custom(String),

    // IO Errors
    #[error("Error: {0}")]
    DaemonFileError(#[from] std::io::Error),

    // Serialization Errors
    #[error("Error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

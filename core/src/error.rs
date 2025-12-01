use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Custom error: {0}")]
    Custom(String),

    #[error("IO error: {0}")]
    IO(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Response Error: {0}")]
    Response(ResponseError),
}

#[derive(Error, Debug)]
pub enum ResponseError {
    #[error("UnexpectedId: expected({expected}) actual({actual})")]
    UnexpectedId{expected:u32, actual:u32},
    #[error("Bad Status: {0}")]
    Status(String)
}


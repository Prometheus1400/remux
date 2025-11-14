use thiserror::Error;

#[derive(Error, Debug)]
pub enum RemuxCLIError {
    #[error("Error interacting with terminal: {0}")]
    IOError(#[from] std::io::Error),

    #[error("Error sending to tokio channel: {0}")]
    SendError(#[from] tokio::sync::mpsc::error::SendError<u8>),

    #[error("Error initializing logger: {0}")]
    LoggerError(#[from] tracing::subscriber::SetGlobalDefaultError),

    #[error("Error using remux lib: {0}")]
    LibError(#[from] remux_core::error::RemuxLibError),

    #[error("Error joining tokio tasks: {0}")]
    JoinError(#[from] tokio::task::JoinError),

    #[error("Error converting bytes to utf8 string: {0}")]
    UTF8Error(#[from] std::str::Utf8Error),

    #[error("Socket Error: {0}")]
    SocketError(String),
}

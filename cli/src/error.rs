use remux_core::messages::{RequestMessage, ResponseMessage};
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Custom Error: {0}")]
    Custom(String),

    #[error("Error using remux lib: {0}")]
    Lib(#[from] remux_core::error::Error),

    #[error("Error initializing logger: {0}")]
    Logger(#[from] tracing::subscriber::SetGlobalDefaultError),

    #[error("Error joining tokio tasks: {0}")]
    Join(#[from] tokio::task::JoinError),

    #[error("IO Error: {0}")]
    IO(#[from] std::io::Error),

    #[error("Error connecting to socket {socket_path}: {source}")]
    ConnectingSocket {
        socket_path: String,
        source: std::io::Error,
    },

    #[error("Error sending message {message}: {source}")]
    SendRequestMessage {
        message: RequestMessage,
        source: remux_core::error::Error,
    },
    #[error("Error sending message {message}: {source}")]
    SendResponseMessage {
        message: ResponseMessage,
        source: remux_core::error::Error,
    },
    // #[error("Error sending to tokio channel: {0}")]
    // SendError(#[from] tokio::sync::mpsc::error::SendError<u8>),
    //
    // #[error("Error converting bytes to utf8 string: {0}")]
    // UTF8Error(#[from] std::str::Utf8Error),
    //
    // #[error("Socket Error: {0}")]
    // SocketError(remux_core::error::Error),
}

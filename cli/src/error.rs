use bytes::Bytes;
use remux_core::messages::RequestMessage;
use thiserror::Error;
use tokio::sync::mpsc::error::SendError;

use crate::actors::{client::ClientEvent, ui::UiEvent};

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

    #[error("Event Send Error: {0}")]
    EventSend(EventSendError),
}

#[derive(Error, Debug)]
pub enum EventSendError {
    #[error("IO send error: {0}")]
    IO(SendError<ClientEvent>),
    #[error("Popup send error: {0}")]
    UI(SendError<UiEvent>),
    #[error("Bytes send error: {0}")]
    Bytes(SendError<Bytes>),
}

impl From<SendError<ClientEvent>> for Error {
    fn from(e: SendError<ClientEvent>) -> Self {
        Self::EventSend(EventSendError::IO(e))
    }
}
impl From<SendError<UiEvent>> for Error {
    fn from(e: SendError<UiEvent>) -> Self {
        Self::EventSend(EventSendError::UI(e))
    }
}
impl From<SendError<Bytes>> for Error {
    fn from(e: SendError<Bytes>) -> Self {
        Self::EventSend(EventSendError::Bytes(e))
    }
}

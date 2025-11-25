use bytes::Bytes;
use remux_core::messages::RequestMessage;
use thiserror::Error;
use tokio::sync::mpsc::error::SendError;

use crate::actors::{client::ClientEvent, lua::{Lua, LuaEvent}, ui::UIEvent, widget_runner::WidgetRunnerEvent};

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

    #[error("Lua error: {0}")]
    Lua(String),

    #[error("Event Send Error: {0}")]
    EventSend(EventSendError),

    #[error("Sync Send Error: {0}")]
    SyncSend(#[from] std::sync::mpsc::SendError<LuaEvent>),
}

impl From<mlua::Error> for Error {
    fn from(e: mlua::Error) -> Self {
        Error::Lua(e.to_string())
    }
}

#[derive(Error, Debug)]
pub enum EventSendError {
    #[error("IO send error: {0}")]
    IO(SendError<ClientEvent>),
    #[error("Popup send error: {0}")]
    UI(SendError<UIEvent>),
    #[error("Bytes send error: {0}")]
    Bytes(SendError<Bytes>),
    #[error("WidgetRunner send error: {0}")]
    WidgetRunner(SendError<WidgetRunnerEvent>),
    // #[error("LuaActor send error: {0}")]
    // LuaActor(SendError<LuaActorEvent>),
}

impl From<SendError<ClientEvent>> for Error {
    fn from(e: SendError<ClientEvent>) -> Self {
        Self::EventSend(EventSendError::IO(e))
    }
}
impl From<SendError<UIEvent>> for Error {
    fn from(e: SendError<UIEvent>) -> Self {
        Self::EventSend(EventSendError::UI(e))
    }
}
impl From<SendError<Bytes>> for Error {
    fn from(e: SendError<Bytes>) -> Self {
        Self::EventSend(EventSendError::Bytes(e))
    }
}
impl From<SendError<WidgetRunnerEvent>> for Error {
    fn from(e: SendError<WidgetRunnerEvent>) -> Self {
        Self::EventSend(EventSendError::WidgetRunner(e))
    }
}
// impl From<SendError<LuaActorEvent>> for Error {
//     fn from(e: SendError<LuaActorEvent>) -> Self {
//         Self::EventSend(EventSendError::LuaActor(e))
//     }
// }

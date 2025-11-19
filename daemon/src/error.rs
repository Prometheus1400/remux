use thiserror::Error;
use tokio::sync::mpsc::error::SendError;

use crate::actors::{
    client::ClientEvent, pane::PaneEvent, pty::PtyEvent, session::SessionEvent,
    session_manager::SessionManagerEvent, window::WindowEvent,
};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Error: {0}")]
    Custom(String),

    #[error("Cannot spawn another remux daemon: {0}")]
    DuplicateProcess(#[from] remux_core::error::Error),

    #[error("IO Error: {0}")]
    IO(#[from] std::io::Error),

    #[error("Error initializing logger: {0}")]
    Logger(#[from] tracing::subscriber::SetGlobalDefaultError),

    #[error("PTY Error: {0}")]
    Pty(#[from] pty::fork::ForkError),

    #[error("Master Error: {0}")]
    Nix(#[from] nix::Error),

    #[error("Master Error: {0}")]
    Master(#[from] pty::fork::MasterError),

    #[error("Slave Error: {0}")]
    Slave(#[from] pty::fork::SlaveError),

    #[error("Join Error: {0}")]
    Join(#[from] tokio::task::JoinError),

    #[error("Event Send Error: {0}")]
    EventSend(EventSendError),
}

#[derive(Error, Debug)]
pub enum EventSendError {
    #[error("Client send error: {0}")]
    ClientSend(SendError<ClientEvent>),
    #[error("Session Manager send error: {0}")]
    SessionManagerSend(SendError<SessionManagerEvent>),
    #[error("Session send error: {0}")]
    SessionSend(SendError<SessionEvent>),
    #[error("Window send error: {0}")]
    WindowSend(SendError<WindowEvent>),
    #[error("Pane send error: {0}")]
    PaneSend(SendError<PaneEvent>),
    #[error("Pty send error: {0}")]
    PtySend(SendError<PtyEvent>),
}

impl From<SendError<ClientEvent>> for EventSendError {
    fn from(e: SendError<ClientEvent>) -> Self {
        EventSendError::ClientSend(e)
    }
}
impl From<SendError<SessionManagerEvent>> for EventSendError {
    fn from(e: SendError<SessionManagerEvent>) -> Self {
        EventSendError::SessionManagerSend(e)
    }
}
impl From<SendError<SessionEvent>> for EventSendError {
    fn from(e: SendError<SessionEvent>) -> Self {
        EventSendError::SessionSend(e)
    }
}
impl From<SendError<WindowEvent>> for EventSendError {
    fn from(e: SendError<WindowEvent>) -> Self {
        EventSendError::WindowSend(e)
    }
}
impl From<SendError<PaneEvent>> for EventSendError {
    fn from(e: SendError<PaneEvent>) -> Self {
        EventSendError::PaneSend(e)
    }
}
impl From<SendError<PtyEvent>> for EventSendError {
    fn from(e: SendError<PtyEvent>) -> Self {
        EventSendError::PtySend(e)
    }
}

// Propagate to top-level
impl From<EventSendError> for Error {
    fn from(e: EventSendError) -> Self {
        Error::EventSend(e)
    }
}

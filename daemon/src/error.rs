use thiserror::Error;
use tokio::sync::mpsc::error::SendError;

use crate::actors::{
    client::ClientConnectionEvent, pane::PaneEvent, pty::PtyEvent, session::SessionEvent,
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
    Client(SendError<ClientConnectionEvent>),
    #[error("Session Manager send error: {0}")]
    SessionManager(SendError<SessionManagerEvent>),
    #[error("Session send error: {0}")]
    Session(SendError<SessionEvent>),
    #[error("Window send error: {0}")]
    Window(SendError<WindowEvent>),
    #[error("Pane send error: {0}")]
    Pane(SendError<PaneEvent>),
    #[error("Pty send error: {0}")]
    Pty(SendError<PtyEvent>),
}

impl From<SendError<ClientConnectionEvent>> for Error {
    fn from(e: SendError<ClientConnectionEvent>) -> Self {
        Self::EventSend(EventSendError::Client(e))
    }
}
impl From<SendError<SessionManagerEvent>> for Error {
    fn from(e: SendError<SessionManagerEvent>) -> Self {
        Self::EventSend(EventSendError::SessionManager(e))
    }
}
impl From<SendError<SessionEvent>> for Error {
    fn from(e: SendError<SessionEvent>) -> Self {
        Self::EventSend(EventSendError::Session(e))
    }
}
impl From<SendError<WindowEvent>> for Error {
    fn from(e: SendError<WindowEvent>) -> Self {
        Self::EventSend(EventSendError::Window(e))
    }
}
impl From<SendError<PaneEvent>> for Error {
    fn from(e: SendError<PaneEvent>) -> Self {
        Self::EventSend(EventSendError::Pane(e))
    }
}
impl From<SendError<PtyEvent>> for Error {
    fn from(e: SendError<PtyEvent>) -> Self {
        Self::EventSend(EventSendError::Pty(e))
    }
}

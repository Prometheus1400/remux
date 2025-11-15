use thiserror::Error;

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

    #[error("Send Error: {0}")]
    Send(#[from] tokio::sync::mpsc::error::SendError<bool>),
}

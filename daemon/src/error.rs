use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Error: {0}")]
    Custom(String),
    
    #[error("Cannot spawn another remux daemon: {0}")]
    DuplicateProcess(#[from] remux_core::error::Error),

    #[error("IO Error: {0}")]
    IOError(#[from] std::io::Error),

    #[error("PTY Error: {0}")]
    PTYError(#[from] pty::fork::ForkError),

    #[error("Master Error: {0}")]
    NixError(#[from] nix::Error),

    #[error("Master Error: {0}")]
    MasterError(#[from] pty::fork::MasterError),

    #[error("Slave Error: {0}")]
    SlaveError(#[from] pty::fork::SlaveError),

    #[error("Join Error: {0}")]
    JoinError(#[from] tokio::task::JoinError),

    #[error("Send Error: {0}")]
    SendError(#[from] tokio::sync::mpsc::error::SendError<bool>),

    #[error("File descriptor error: {0}")]
    FDError(String),

    #[error("Unix Socket Error: {0}")]
    UnixSocketError(remux_core::error::Error),

    #[error("UTF8Error: {0}")]
    StringError(#[from] std::string::FromUtf8Error),

    #[error("NulError: {0}")]
    CStringError(#[from] std::ffi::NulError),
}

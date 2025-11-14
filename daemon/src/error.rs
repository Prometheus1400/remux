use thiserror::Error;

#[derive(Error, Debug)]
pub enum RemuxDaemonError {
    #[error("Cannot spawn another remux daemon: {0}")]
    DuplicateProcess(#[from] remux_core::error::RemuxLibError),

    #[error("IO Error: {0}")]
    IOError(#[from] std::io::Error),

    #[error("PTY Error: {0}")]
    PTYError(#[from] pty::fork::ForkError),

    #[error("Master Error: {0}")]
    NixError(#[from] nix::Error),

    #[error("Master Error: {0}")]
    MasterError(#[from] pty::fork::MasterError),

    #[error("Generic Master Error: {0}")]
    GenericMasterError(String),

    #[error("Generic Unix Error: {0}")]
    GenericUnixError(String),

    #[error("Slave Error: {0}")]
    SlaveError(#[from] pty::fork::SlaveError),

    #[error("Join Error: {0}")]
    JoinError(#[from] tokio::task::JoinError),

    #[error("Send Error: {0}")]
    SendError(#[from] tokio::sync::mpsc::error::SendError<bool>),

    #[error("File descriptor error: {0}")]
    FDError(String),

    #[error("Socket Error: {0}")]
    SocketError(String)
}

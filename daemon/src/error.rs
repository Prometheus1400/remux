use thiserror::Error;
use tokio::task::JoinError;

#[derive(Error, Debug)]
pub enum RemuxDaemonError {
    #[error("Cannot spawn another remux daemon: {0}")]
    DuplicateProcess(#[from] remux_core::error::RemuxLibError),

    #[error("IO Error: {0}")]
    IOError(#[from] std::io::Error),

    #[error("PTY Error: {0}")]
    PTYError(#[from] pty::fork::ForkError),

    #[error("Master Error: {0}")]
    NixPTYError(#[from] nix::Error),

    #[error("Master Error: {0}")]
    MasterError(#[from] pty::fork::MasterError),

    #[error("Slave Error: {0}")]
    SlaveError(#[from] pty::fork::SlaveError),

    #[error("Join Error: {0}")]
    JoinError(#[from] JoinError)

}

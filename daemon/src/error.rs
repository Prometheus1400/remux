use thiserror::Error;

#[derive(Error, Debug)]
pub enum RemuxDaemonError {
    #[error("Cannot spawn another remux daemon: {0}")]
    DuplicateProcess(#[from] remux_core::error::RemuxLibError),

    #[error("IO Error: {0}")]
    IOError(#[from] std::io::Error),
}

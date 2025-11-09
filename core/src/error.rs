use thiserror::Error;

#[derive(Error, Debug)]
pub enum RemuxLibError {
    #[error("IO error: {0}")]
    DaemonFileError(#[from] std::io::Error),
}

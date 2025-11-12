use thiserror::Error;

#[derive(Error, Debug)]
pub enum RemuxCLIError {
    #[error("Error communicating with daemon: {0}")]
    CommunicationError(String),

    #[error("Error interacting with terminal: {0}")]
    TerminalError(std::io::Error),
}

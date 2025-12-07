#![allow(unused_imports)]
use tokio::task::JoinHandle;
pub use tracing::{debug, error, info, instrument, trace, warn};

pub type DaemonTask = JoinHandle<std::result::Result<(), color_eyre::eyre::Error>>;

pub use color_eyre::eyre::{Error, Result};

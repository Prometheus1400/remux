#![allow(unused)]

use tokio::task::JoinHandle;
pub use tracing::{debug, error, info, instrument, trace, warn};

pub type CliTask = JoinHandle<Result<()>>;

pub use color_eyre::eyre::{Error, Result};

#![allow(unused_imports)]
use tokio::task::JoinHandle;
pub use tracing::{debug, error, info, instrument, trace, warn};

pub use crate::{
    error::{Error, Result},
    handle_method,
};
pub type DaemonTask = JoinHandle<std::result::Result<(), Error>>;

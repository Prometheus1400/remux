use tokio::task::JoinHandle;

use crate::error::Error;

pub type DaemonTask = JoinHandle<std::result::Result<(), Error>>;

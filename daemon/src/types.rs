use tokio::task::JoinHandle;

use crate::error::Error;

pub type NoResTask = JoinHandle<std::result::Result<(), Error>>;

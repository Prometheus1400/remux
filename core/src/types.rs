use tokio::task::JoinHandle;

use crate::error::Error;

// pub type NoResTask = JoinHandle<std::result::Result<(), Error>>;
pub type BackgroundTask<E> = JoinHandle<std::result::Result<(), E>>;

use std::sync::Arc;

use tokio::{net::UnixStream, sync::Mutex};

/// context associated with an attached client
/// meaning this client sent the Connect message
pub struct Context {
    stream: Arc<Mutex<UnixStream>>,
}

impl Context {
    pub fn new(stream: UnixStream) -> Self {
        Self {
            stream: Arc::new(Mutex::new(stream))
        }
    }
}

impl Clone for Context {
    fn clone(&self) -> Self {
        Self { stream: self.stream.clone() }
    }
}

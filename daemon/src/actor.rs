
use crate::error::Result;

use crate::types::DaemonTask;

pub trait ActorHandle: Clone { 
    async fn kill(&self) -> Result<()>;
    fn is_alive(&self) -> bool;
}

pub trait Actor<Handle : ActorHandle> {
    fn run(self) -> Result<Handle>;
}

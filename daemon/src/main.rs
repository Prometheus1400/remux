mod daemon;
mod error;

use daemon::RemuxDaemon;

use crate::error::RemuxDaemonError;

async fn run() -> Result<(), RemuxDaemonError> {
    let daemon = RemuxDaemon::new()?;
    daemon.listen().await
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

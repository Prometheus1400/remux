mod daemon;
mod types;
mod error;
mod pane;
mod pty;
mod session;

use daemon::RemuxDaemon;
use tracing::{error, info};
use tracing_subscriber::FmtSubscriber;

use crate::error::Result;

async fn run() -> Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(tracing::Level::DEBUG)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("daemon started");
    let daemon = RemuxDaemon::new()?;
    daemon.listen().await
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        error!("{e}");
        std::process::exit(1);
    }
}

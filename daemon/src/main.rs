mod daemon;
mod error;

use daemon::RemuxDaemon;
use tracing::{error, info, instrument};
use tracing_subscriber::FmtSubscriber;

use crate::error::RemuxDaemonError;

async fn run() -> Result<(), RemuxDaemonError> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(tracing::Level::DEBUG)
        .finish();
    tracing::subscriber::set_global_default(subscriber).unwrap();

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

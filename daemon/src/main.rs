mod actors;
mod control_signals;
mod daemon;
mod error;
mod prelude;
mod input_parser;

use daemon::RemuxDaemon;
use tracing_subscriber::FmtSubscriber;

use crate::prelude::*;

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

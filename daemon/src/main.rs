mod actors;
mod control_signals;
mod daemon;
mod error;
mod input_parser;
mod prelude;

use daemon::RemuxDaemon;
use tracing_subscriber::{EnvFilter, FmtSubscriber};

use crate::prelude::*;

#[tokio::main]
async fn main() {
    if let Err(e) = setup_logging() {
        eprintln!("{e}");
        std::process::exit(1);
    }
    if let Err(e) = run().await {
        error!("{e}");
        std::process::exit(1);
    }
}

fn setup_logging() -> Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug"));
    let subscriber = FmtSubscriber::builder().with_env_filter(filter).finish();
    tracing::subscriber::set_global_default(subscriber)?;
    Ok(())
}

#[instrument]
async fn run() -> Result<()> {
    let daemon = RemuxDaemon::new()?;
    info!("daemon started");
    daemon.listen().await
}

mod actors;
mod control_signals;
mod daemon;
mod layout;
mod prelude;

use daemon::RemuxDaemon;

use crate::prelude::*;

#[tokio::main]
async fn main() {
    if let Err(e) = setup_logging() {
        eprintln!("{e}");
        std::process::exit(1);
    }
    color_eyre::install().unwrap();
    if let Err(e) = run().await {
        error!("{e}");
        std::process::exit(1);
    }
}

fn setup_logging() -> Result<()> {
    use tracing_error::ErrorLayer;
    use tracing_subscriber::{EnvFilter, FmtSubscriber, fmt::format::FmtSpan, layer::SubscriberExt};

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug"));
    let subscriber = FmtSubscriber::builder()
        .with_env_filter(filter)
        .with_span_events(FmtSpan::NONE)
        .with_line_number(false)
        .with_file(false)
        .with_target(false)
        .with_level(true)
        .with_thread_ids(false)
        .finish()
        .with(ErrorLayer::default());

    tracing::subscriber::set_global_default(subscriber)?;
    Ok(())
}

#[instrument(err)]
async fn run() -> Result<()> {
    let daemon = RemuxDaemon::new()?;
    info!("daemon started");
    daemon.listen().await
}

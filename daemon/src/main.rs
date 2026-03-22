mod actors;
mod cell;
mod control_signals;
mod daemon;
mod input_parser;
mod layout;
mod lua;
mod prelude;
mod render;

use color_eyre::eyre::WrapErr;
use daemon::RemuxDaemon;

use crate::prelude::*;

#[tokio::main]
async fn main() {
    if let Err(e) = setup_logging() {
        eprintln!("{e}");
        std::process::exit(1);
    }
    if let Err(e) = color_eyre::install() {
        eprintln!("failed to install color_eyre: {e}");
        std::process::exit(1);
    }
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

    tracing::subscriber::set_global_default(subscriber).wrap_err("failed to install daemon tracing subscriber")?;
    Ok(())
}

#[instrument(err)]
async fn run() -> Result<()> {
    let daemon = RemuxDaemon::new()?;
    info!("daemon started");
    daemon.listen().await
}

mod app;
mod args;
mod prelude;
mod tasks;

use std::{fs::File, path::Path};

use clap::Parser;
use color_eyre::eyre::WrapErr;
use crossterm::terminal::{disable_raw_mode, size as terminal_size};
use remux_core::{
    comm,
    daemon_utils::get_sock_path,
    messages::{
        CliRequestMessage, RequestBuilder,
        request::{self, Attach},
    },
};
use tokio::net::UnixStream;
use uuid::Uuid;

use crate::{
    app::App,
    args::{Args, Commands},
    prelude::*,
};

#[tokio::main]
async fn main() {
    if let Err(e) = color_eyre::install() {
        eprintln!("failed to install color_eyre: {e}");
        std::process::exit(1);
    }
    let cli = Args::parse();
    match setup_logging() {
        Ok(_guard) => {
            if let Err(e) = run(cli.command).await {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        Err(e) => {
            if let Err(e) = disable_raw_mode() {
                eprintln!("error disabling raw mode: {e}");
                eprintln!("terminal may still be in raw mode!!! You can run 'stty sane' to reset it.");
            }
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
}

fn setup_logging() -> Result<tracing_appender::non_blocking::WorkerGuard> {
    setup_logging_at(Path::new("./logs/remux-cli.log"))
}

fn setup_logging_at(log_path: &Path) -> Result<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_appender::non_blocking;
    use tracing_error::ErrorLayer;
    use tracing_subscriber::{EnvFilter, FmtSubscriber, fmt::format::FmtSpan, layer::SubscriberExt};

    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent).wrap_err("failed to create CLI log directory")?;
    }

    let file = File::create(log_path).wrap_err("failed to create CLI log file")?;
    let (non_blocking_writer, guard) = non_blocking(file);

    // Environment filter
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug"));

    // Build the subscriber
    let subscriber = FmtSubscriber::builder()
        .with_env_filter(filter)
        .with_span_events(FmtSpan::NONE)
        .with_line_number(false)
        .with_file(false)
        .with_target(false)
        .with_level(true)
        .with_thread_ids(false)
        .with_writer(non_blocking_writer)
        .finish()
        .with(ErrorLayer::default());

    tracing::subscriber::set_global_default(subscriber).wrap_err("failed to install CLI tracing subscriber")?;

    Ok(guard)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::setup_logging_at;

    fn unique_log_path() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        std::env::temp_dir()
            .join(format!("remux-cli-test-{nonce}"))
            .join("logs")
            .join("remux-cli.log")
    }

    #[test]
    fn setup_logging_creates_missing_log_directory() {
        let log_path = unique_log_path();
        let log_dir = log_path.parent().expect("log path should have parent");
        assert!(!log_dir.exists());

        let _guard = setup_logging_at(&log_path).expect("setup logging should create log path");

        assert!(log_path.exists());

        let root = log_dir.parent().expect("logs dir should have test root");
        fs::remove_dir_all(root).expect("cleanup temp log directory");
    }
}

#[instrument]
async fn connect() -> Result<UnixStream> {
    let socket_path = get_sock_path()?;
    debug!(path=?socket_path, "Connecting to unix socket");
    let stream = UnixStream::connect(socket_path.clone())
        .await
        .wrap_err_with(|| format!("failed to connect to unix socket at {}", socket_path.display()))?;
    Ok(stream)
}

#[instrument]
async fn run(command: Commands) -> Result<()> {
    let stream = connect().await?;
    debug!("Running command");
    match command {
        Commands::Attach { session_name } => {
            let (cols, rows) = terminal_size().wrap_err("failed to read terminal size for attach request")?;
            attach(
                stream,
                RequestBuilder::default()
                    .body(request::Attach {
                        id: Uuid::new_v4(),
                        session_name,
                        create: true,
                        rows,
                        cols,
                    })
                    .build(),
            )
            .await
        }
        _ => todo!(),
    }
}

#[instrument(skip(stream))]
async fn attach(mut stream: UnixStream, attach_request: CliRequestMessage<Attach>) -> Result<()> {
    debug!("Sending attach request");
    let res = comm::send_and_recv_message(&mut stream, &attach_request).await?;
    debug!(response=?res, "Recieved attach response");

    debug!("Starting app");
    let mut app = App::new(attach_request.body.id, stream);
    app.run().await?;
    debug!("App terminated");
    disable_raw_mode()?;
    Ok(())
}

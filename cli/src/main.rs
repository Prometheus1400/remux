mod app;
mod args;
mod input_parser;
mod prelude;
mod states;
mod tasks;
mod ui;

use std::fs::File;

use clap::Parser;
use ratatui::crossterm::terminal::disable_raw_mode;
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
    color_eyre::install().unwrap();
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
    use tracing_appender::non_blocking;
    use tracing_error::ErrorLayer;
    use tracing_subscriber::{EnvFilter, FmtSubscriber, fmt::format::FmtSpan, layer::SubscriberExt};
    // Create the log file
    let file = File::create("./logs/remux-cli.log")?;
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

    tracing::subscriber::set_global_default(subscriber)?;

    Ok(guard)
}

#[instrument]
async fn connect() -> Result<UnixStream> {
    let socket_path = get_sock_path()?;
    debug!(path=?socket_path, "Connecting to unix socket");
    let stream = UnixStream::connect(socket_path.clone()).await?;
    Ok(stream)
}

#[instrument]
async fn run(command: Commands) -> Result<()> {
    let stream = connect().await?;
    debug!("Running command");
    match command {
        Commands::Attach { session_name } => {
            attach(
                stream,
                RequestBuilder::default()
                    .body(request::Attach {
                        id: Uuid::new_v4(),
                        session_name,
                        create: true,
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
    debug!(daemon_state=?res.initial_daemon_state, "Recieved initial daemon state");

    debug!("Starting app");
    let mut app = App::new(attach_request.body.id, stream, res.initial_daemon_state);
    app.run().await?;
    debug!("App terminated");
    disable_raw_mode()?;
    Ok(())
}

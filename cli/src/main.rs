mod actors;
pub mod app;
mod args;
mod error;
mod input;
mod input_parser;
mod prelude;
mod states;
mod utils;
mod widgets;

use clap::Parser;
use ratatui::crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use remux_core::{
    comm,
    daemon_utils::get_sock_path,
    messages::{
        CliRequestMessage, RequestBuilder,
        request::{self, Attach},
    },
};
use tokio::net::UnixStream;

use crate::{
    app::App,
    args::{Args, Commands},
    error::{Error, Result},
    prelude::*,
};

#[tokio::main]
async fn main() {
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
    use tracing_appender::{non_blocking, rolling};
    use tracing_subscriber::{EnvFilter, fmt};

    let file_appender = rolling::daily("logs", "remux-cli.log");
    let (non_blocking, guard) = non_blocking(file_appender);

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug"));

    let subscriber = fmt().with_writer(non_blocking).with_env_filter(env_filter).finish();
    tracing::subscriber::set_global_default(subscriber)?;
    Ok(guard)
}

#[instrument]
async fn connect() -> Result<UnixStream> {
    let socket_path = get_sock_path()?;
    debug!("Connecting to {:?}", socket_path);
    UnixStream::connect(socket_path.clone())
        .await
        .map_err(|source| Error::ConnectingSocket {
            socket_path: socket_path.to_string_lossy().into_owned(),
            source,
        })
}

#[instrument]
async fn run(command: Commands) -> Result<()> {
    let stream = connect().await?;
    debug!("Running command: {:?}", command);
    match command {
        Commands::Attach { session_id } => {
            attach(
                stream,
                RequestBuilder::default()
                    .body(request::Attach {
                        session_id,
                        create: true,
                    })
                    .build(),
            )
            .await
        }
        _ => todo!(),
    }
}

#[instrument(skip(stream, attach_request))]
async fn attach(mut stream: UnixStream, attach_request: CliRequestMessage<Attach>) -> Result<()> {
    debug!("Sending attach request: {:?}", attach_request);
    let res = comm::send_and_recv_message(&mut stream, &attach_request).await?;
    debug!("Recieved attach response: {:?}", res);
    debug!("Recieved initial daemon state: {:?}", res.initial_daemon_state);

    let mut app = App::new(stream);
    app.run().await

    // enable_raw_mode()?;
    // debug!("Enabled raw mode");
    // if let Ok(task) = Client::spawn(stream, res.initial_daemon_state) {
    //     match task.await {
    //         Ok(Err(e)) => {
    //             error!("Error joining client task: {e}");
    //         }
    //         Err(e) => {
    //             error!("Error joining client task: {e}");
    //         }
    //         _ => {}
    //     }
    // }
    // disable_raw_mode()?;
    // debug!("Disabled raw mode");
    // Ok(())
}

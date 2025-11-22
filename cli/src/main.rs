mod actors;
mod args;
mod error;
mod widgets;
mod prelude;

use clap::Parser;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use remux_core::{
    communication, daemon_utils::get_sock_path, messages::{RequestMessage, ResponseBody, ResponseMessage},
};
use tokio::net::UnixStream;

use crate::{
    actors::io::Client,
    args::{Args, Commands, SessionCommands},
    error::{Error, Result},
};
use crate::prelude::*;

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

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("trace"));

    let subscriber = fmt()
        .with_writer(non_blocking)
        .with_env_filter(env_filter)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;
    Ok(guard)
}

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

async fn handle_session_command(mut stream: UnixStream, command: SessionCommands) -> Result<()> {
    let req: RequestMessage = command.into();
    let res: ResponseMessage = communication::send_and_recv(&mut stream, &req).await?;
    match res.body {
        ResponseBody::SessionsList { sessions } => {
            println!("{sessions:?}");
        }
    }
    Ok(())
}

async fn run(command: Commands) -> Result<()> {
    let stream = connect().await?;
    match command {
        a @ Commands::Attach { .. } => attach(stream, a.into()).await,
        Commands::Session { action } => handle_session_command(stream, action).await,
    }
}

async fn attach(mut stream: UnixStream, attach_message: RequestMessage) -> Result<()> {
    debug!("Sending attach request");
    communication::write_message(&mut stream, &attach_message)
        .await
        .map_err(|source| Error::SendRequestMessage {
            message: attach_message,
            source,
        })?;
    debug!("Sent attach request successfully");
    enable_raw_mode()?;
    if let Ok(task) = Client::spawn(stream) {
        task.await?;
    }
    disable_raw_mode()?;
    Ok(())
}

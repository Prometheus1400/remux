mod error;

use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use remux_core::{
    daemon_utils::get_sock_path,
    messages::{self, RemuxDaemonRequest},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};
use tracing::{debug, error, info, trace};

use crate::error::{Error, Result};

#[tokio::main]
async fn main() {
    match setup_logging() {
        Ok(_guard) => {
            if let Err(e) = run().await {
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

async fn run() -> Result<()> {
    let socket_path = get_sock_path()?;
    debug!("Connecting to {:?}", socket_path);
    let mut stream = UnixStream::connect(socket_path.clone())
        .await
        .map_err(|source| Error::ConnectingSocket {
            socket_path: socket_path.to_string_lossy().into_owned(),
            source,
        })?;
    debug!("Sending connect request");
    let message = RemuxDaemonRequest::Connect {
        session_id: 1,
        create: true,
    };
    messages::write_message(&mut stream, &message)
        .await
        .map_err(|source| Error::SendMessage { message, source })?;
    debug!("Sent connect request successfully");

    let (send_to_tcp, mut recv_for_tcp) = tokio::sync::mpsc::unbounded_channel::<u8>();
    let (send_cancel_stdin_task, mut recv_cancel_stdin_task) =
        tokio::sync::mpsc::unbounded_channel::<u8>();

    enable_raw_mode()?;
    let tcp_task: tokio::task::JoinHandle<std::result::Result<_, Error>> =
        tokio::spawn(async move {
            let mut stdout = tokio::io::stdout();
            let mut buf = [0u8; 1024];
            loop {
                tokio::select! {
                    Ok(n) = stream.read(&mut buf) => {
                        if n > 0 {
                            trace!("Read {n} bytes from daemon");
                            stdout.write_all(&buf[..n]).await?;
                            stdout.flush().await?;
                        } else {
                            // stream closed
                            break;
                        }
                    },
                    b_opt = recv_for_tcp.recv() => {
                        match b_opt{
                            Some(b) => {
                                trace!("Sending byte '{}' to tcp stream", b);
                                stream.write_u8(b).await?;
                            },
                            None => {
                                debug!("channel closed");
                                break;
                            },
                        }
                    }
                }
            }
            info!("closing tcp task");
            send_cancel_stdin_task
                .send(1)
                .map_err(|_| Error::Custom("can't send to cancel stdin channel".to_owned()))?;
            Ok(())
        });

    let stdin_task: tokio::task::JoinHandle<std::result::Result<_, Error>> =
        tokio::spawn(async move {
            let mut stdin = tokio::io::stdin(); // read here
            let mut buf = [0u8; 1024];
            loop {
                tokio::select! {
                    // after all tasks exit the read is still blocked under the hood so it needs some
                    // keypress to trigger the release
                    stdin_res = stdin.read(&mut buf) => {
                        match stdin_res {
                            Ok(n) if n > 0 => {
                                trace!("Sending {n} bytes to daemon");
                                for &b in &buf[..n] {
                                    send_to_tcp.send(b).map_err(|_| {
                                        Error::Custom("can't send to tcp channel".to_owned())
                                    })?;
                                }
                            }
                            Ok(_) => {
                                debug!("stream closed");
                                break; // stream is closed
                            }
                            Err(e) => {
                                error!("Error reading from stdin: {e}");
                                continue;
                            }
                        }
                    },
                    Some(_) = recv_cancel_stdin_task.recv() => {
                        break;
                    }
                }
            }
            info!("closing stdin task");
            Ok(())
        });

    if let Err(e) = tokio::try_join!(tcp_task, stdin_task) {
        error!("error joining tokio tasks: {e}");
        disable_raw_mode()?;
        Err(e.into())
    } else {
        disable_raw_mode()?;
        Ok(())
    }
}

mod error;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};

use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use remux_core::{
    constants,
    messages::{self, RemuxDaemonRequest},
};

use error::RemuxCLIError::{self};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tracing::{debug, error, trace};

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

fn setup_logging() -> Result<tracing_appender::non_blocking::WorkerGuard, RemuxCLIError> {
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

async fn run() -> Result<(), RemuxCLIError> {
    let addr = SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), constants::PORT);
    debug!("Connecting to {addr}");
    let mut stream = TcpStream::connect(addr).await?;
    debug!("Sending connect request");
    messages::write_message(&mut stream, RemuxDaemonRequest::Connect).await?;
    debug!("Sent connect request successfully");

    let (send_to_tcp, mut recv_for_tcp) = tokio::sync::mpsc::unbounded_channel::<u8>();
    enable_raw_mode()?;

    let tcp_task = tokio::spawn(async move {
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
                        return Ok(());
                    }
                },
                Some(b) = recv_for_tcp.recv() => {
                    trace!("Sending byte '{}' to tcp stream", b);
                    stream.write_u8(b).await?;
                }
            }
        }
        #[allow(unreachable_code)]
        Ok::<(), RemuxCLIError>(())
    });

    let stdin_task = tokio::spawn(async move {
        let mut stdin = tokio::io::stdin();
        let mut buf = [0u8; 1024];
        loop {
            match stdin.read(&mut buf).await {
                Ok(n) if n > 0 => {
                    trace!("Sending {n} bytes to daemon");
                    for &b in &buf[..n] {
                        send_to_tcp.send(b)?;
                    }
                }
                Ok(_) => {
                    continue;
                }
                Err(e) => {
                    error!("Error reading from stdin: {e}");
                    continue;
                }
            }
        }
        #[allow(unreachable_code)]
        Ok::<(), RemuxCLIError>(())
    });

    if let Err(e) = tokio::try_join!(tcp_task, stdin_task) {
        error!("error joining tokio tasks: {e}");
        disable_raw_mode()?;
        return Err(e.into());
    }

    disable_raw_mode()?;
    Ok(())
}

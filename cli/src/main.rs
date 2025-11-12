mod error;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};

use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use remux_core::{
    constants,
    messages::{self, RemuxDaemonRequest},
};

use error::RemuxCLIError::{self, CommunicationError, TerminalError};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), RemuxCLIError> {
    let addr = SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), constants::PORT);
    let mut stream = TcpStream::connect(addr).await.unwrap();
    messages::write_message(&mut stream, RemuxDaemonRequest::Connect)
        .await
        .map_err(|_e| CommunicationError("error sending connect message".to_owned()))?;

    let (send_to_tcp, mut recv_for_tcp) = tokio::sync::mpsc::unbounded_channel::<u8>();

    enable_raw_mode().map_err(|e| TerminalError(e))?;

    let tcp_task = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        let mut buf = [0u8; 1024];
        loop {
            tokio::select! {
                Ok(n) = stream.read(&mut buf) => {
                    stdout.write_all(&buf[..n]).await.expect("should be able to write u8 to stdout");
                    stdout.flush().await.unwrap();
                },
                Some(b) = recv_for_tcp.recv() => {
                    stream.write_u8(b).await.expect("should be able to send u8");
                }
            }
        }
    });

    let stdin_task = tokio::spawn(async move {
        let mut stdin = tokio::io::stdin();
        let mut buf = [0u8; 1024];

        loop {
            match stdin.read(&mut buf).await {
                Ok(n) if n > 0 => {
                    for &b in &buf[..n] {
                        if b == 0x03 {
                            // Ctrl+C
                            disable_raw_mode().map_err(|e| TerminalError(e)).unwrap();
                            break;
                        }
                        send_to_tcp.send(b).unwrap();
                    }
                }
                Ok(_) => {
                    continue;
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    continue;
                }
            }
        }
    });

    tokio::try_join!(tcp_task, stdin_task);
    Ok(())
}

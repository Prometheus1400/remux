use std::fs::File;
use std::net::IpAddr;
use std::net::Ipv6Addr;
use std::net::SocketAddr;

use crate::error::RemuxDaemonError;
use remux_core::constants;
use remux_core::daemon_utils;
use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub struct RemuxDaemon {
    port: u16, // port the daemon is listening on for IPC
    _daemon_file: File, // daemon must hold the exclusive file lock while it is alive and running
}

impl RemuxDaemon {
    /// Makes sure there can only ever be once instance at the
    /// process level through use of OS level file locks
    pub fn new() -> Result<Self, RemuxDaemonError> {
        Ok(Self {
            port: constants::PORT,
            _daemon_file: daemon_utils::lock_daemon_file()?,
        })
    }

    pub async fn listen(&self) -> Result<(), RemuxDaemonError> {
        let listener = TcpListener::bind(self.get_sock_addr()).await?;
        loop {
            let (stream, addr) = listener.accept().await?;
            println!("accepting connection from: {}", addr);
            tokio::spawn(async move {
                handle_communication(stream).await;
            });
        }
    }

    fn get_sock_addr(&self) -> SocketAddr {
        SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), self.port)
    }
}

async fn handle_communication(mut stream: TcpStream) {
    let mut buf = [0u8; 1024];
    loop {
        let n = stream.read(&mut buf).await;        
    }
}

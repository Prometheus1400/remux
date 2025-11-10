use nix::pty;
use nix::unistd;
use std::fs::File;
use std::net::IpAddr;
use std::net::Ipv6Addr;
use std::net::SocketAddr;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::unix::AsyncFd;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::error::RemuxDaemonError;
use remux_core::constants;
use remux_core::daemon_utils;
use remux_core::messages;
use remux_core::messages::RemuxDaemonRequest;
use tokio::net::TcpListener;
use tokio::net::TcpStream;

pub struct RemuxDaemon {
    port: u16,          // port the daemon is listening on for IPC
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
    loop {
        let message: RemuxDaemonRequest = messages::read_message(&mut stream).await.unwrap();
        match message {
            RemuxDaemonRequest::Connect => {
                run_pty(stream).await;
                break;
            }
            RemuxDaemonRequest::Disconnect => todo!(),
        }
    }
}

async fn run_pty(mut stream: TcpStream) -> Result<(), RemuxDaemonError> {
    use pty::{
        ForkptyResult,
        ForkptyResult::{Child, Parent},
        forkpty,
    };
    use tokio::sync::mpsc;

    let fork_result: ForkptyResult;
    unsafe {
        fork_result = forkpty(None, None)?;
    }
    match fork_result {
        // one tokio task that just wraps the master FD
        // one task that just wraps the stream
        //
        // they can communicate via channels !!
        Parent { child, master } => {
            let (send_to_pty, mut recv_for_pty) = mpsc::channel::<Vec<u8>>(64);
            let (send_to_tcp, mut recv_for_tcp) = mpsc::channel::<Vec<u8>>(64);
            let async_fd = AsyncFd::new(master)?;

            let pty_fd_task = tokio::spawn(async move {
                loop {
                    tokio::select! {
                        // read from PTY
                        Ok(mut guard) = async_fd.readable() => {
                            let mut buf = [0u8; 1024];
                            let data = guard.try_io(|fd| {
                                match unistd::read(fd.get_ref(), &mut buf) {
                                    Ok(n) => Ok(buf[..n].to_vec()),
                                    Err(nix::errno::Errno::EAGAIN) => Ok(Vec::new()), // no data yet
                                    Err(e) => Err(e.into()),
                                }
                            }).expect("expected data to be read from master fd");
                            match data {
                                Ok(buf) if !buf.is_empty() => {
                                    send_to_tcp.send(buf).await.unwrap();
                                }
                                _ => {}
                            };
                        },
                        // write to PTY
                        Some(data) = recv_for_pty.recv() => {
                            let mut guard = async_fd.writable().await.unwrap();
                            guard.try_io(|fd| {
                                match unistd::write(fd.get_ref(), &data) {
                                    Ok(n) => {},
                                    Err(_) => todo!(),
                                };
                                Ok(())
                            }).unwrap();
                        }
                    }
                }
            });

            let tcp_task = tokio::spawn(async move {
                let mut buf = [0u8; 1024];
                loop {
                    tokio::select! {
                        Ok(n) = stream.read(&mut buf) => {
                            send_to_pty.send(buf[..n].to_vec()).await.unwrap();
                        },
                        Some(mut data) = recv_for_tcp.recv() => {
                            stream.write_all(&mut data).await.unwrap();
                        }
                    }
                }
            });

            tokio::try_join!(pty_fd_task, tcp_task)?;
            Ok(())
        }
        Child => {
            Command::new("/bin/bash").spawn()?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod test {
    #[tokio::test]
    async fn name() {
    }
}

use nix::{
    libc::{F_GETFL, F_SETFL, O_NONBLOCK, fcntl},
    pty,
    unistd::{self, execvp},
};
use pty::{
    ForkptyResult::{Child, Parent},
    forkpty,
};
use std::{
    ffi::CString,
    fs::File,
    net::{IpAddr, Ipv6Addr, SocketAddr},
    os::fd::{AsFd, AsRawFd},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, unix::AsyncFd},
    net::{TcpListener, TcpStream},
    sync::mpsc,
};
use tracing::{error, info, instrument};

use crate::error::RemuxDaemonError::{self, GenericMasterError};
use remux_core::{
    constants, daemon_utils,
    messages::{self, RemuxDaemonRequest},
};

#[derive(Debug)]
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

    #[instrument(skip(self))]
    pub async fn listen(&self) -> Result<(), RemuxDaemonError> {
        let listener = TcpListener::bind(self.get_sock_addr()).await?;
        loop {
            let (stream, addr) = listener.accept().await?;
            info!("accepting connection from: {}", addr);
            tokio::spawn(async move {
                if let Err(e) = handle_communication(stream).await {
                    error!("{e}");
                }
            });
        }
    }

    fn get_sock_addr(&self) -> SocketAddr {
        SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), self.port)
    }
}

#[instrument]
async fn handle_communication(mut stream: TcpStream) -> Result<(), RemuxDaemonError> {
    loop {
        let message: RemuxDaemonRequest = messages::read_message(&mut stream).await.unwrap();
        match message {
            RemuxDaemonRequest::Connect => {
                run_pty(stream).await?;
                return Ok(());
            }
            RemuxDaemonRequest::Disconnect => todo!(),
        }
    }
}

#[instrument]
async fn run_pty(mut stream: TcpStream) -> Result<(), RemuxDaemonError> {
    // setsid()?;
    let fork_result = unsafe { forkpty(None, None)? };
    match fork_result {
        // one tokio task that just wraps the master FD
        // one task that just wraps the stream
        //
        // they can communicate via channels !!
        Parent { child, master } => {
            info!("parent PID: {}", master.as_fd().as_raw_fd());
            info!("child PID: {}", child.as_raw());
            let (send_to_pty, mut recv_for_pty) = mpsc::unbounded_channel::<Vec<u8>>();
            let (send_to_tcp, mut recv_for_tcp) = mpsc::unbounded_channel::<Vec<u8>>();

            let fd = master.as_raw_fd();
            unsafe {
                // Get current flags
                let flags = fcntl(fd, F_GETFL);
                // Set non-blocking flag
                let new_flags = flags | O_NONBLOCK;
                fcntl(fd, F_SETFL, new_flags);
            }
            let async_fd = AsyncFd::new(master)?;

            let pty_fd_task = tokio::spawn(async move {
                loop {
                    tokio::select! {
                        // read from PTY
                        Ok(mut guard) = async_fd.readable() => {
                            let mut buf = [0u8; 1024];
                            match guard.try_io(|fd| unistd::read(fd.get_ref(), &mut buf).map_err(|e| e.into())) {
                                Ok(Ok(n)) if n > 0 => {
                                    send_to_tcp.send(buf[..n].to_vec()).map_err(|_e| GenericMasterError("couldn't send to tcp".into()))?;
                                },
                                Ok(Ok(_)) => {
                                    break;
                                },
                                Ok(Err(e)) => {
                                    error!("Error reading: {e}");
                                },
                                Err(_would_block) => {continue;},
                            }
                        },
                        // write to PTY
                        Some(data) = recv_for_pty.recv() => {
                            let mut guard = async_fd.writable().await?;
                            let _res = guard.try_io(|fd| {
                                match unistd::write(fd.get_ref(), &data) {
                                    Ok(n) => println!("wrote {n} bytes to pty"),
                                    Err(e) => error!("error writing to pty: {}", e),
                                };
                                Ok(())
                            }).map_err(|_e| GenericMasterError("failed to write to master fd".to_owned()))?;
                        }
                    }
                }
                #[allow(unreachable_code)]
                Ok::<(), RemuxDaemonError>(())
            });

            let tcp_task = tokio::spawn(async move {
                let mut buf = [0u8; 1024];
                loop {
                    tokio::select! {
                        Ok(n) = stream.read(&mut buf) => {
                            if n > 0 {
                                send_to_pty.send(buf[..n].to_vec()).map_err(|_e| GenericMasterError("could not send to pty".to_owned()))?;
                            } else {
                                info!("Tcp client disconnected");
                                break;
                            }
                        },
                        Some(data) = recv_for_tcp.recv() => {
                            info!("writing to tcp: {}", String::from_utf8(data.clone()).unwrap());
                            stream.write_all(&data).await.unwrap();
                        }
                    }
                }
                #[allow(unreachable_code)]
                Ok::<(), RemuxDaemonError>(())
            });

            let (pty_res, tcp_res) = tokio::try_join!(pty_fd_task, tcp_task)?;
            if pty_res.is_err() {
                error!("error in pty task: {}", pty_res.err().unwrap());
            }
            if tcp_res.is_err() {
                error!("error in tcp task: {}", tcp_res.err().unwrap());
            }
            Ok(())
        }
        Child => {
            // exec bash
            let cmd = CString::new("/bin/bash").unwrap();
            execvp(&cmd, std::slice::from_ref(&cmd)).unwrap();
            unreachable!();
        }
    }
}

#[cfg(test)]
mod test {
    #[tokio::test]
    async fn name() {}
}

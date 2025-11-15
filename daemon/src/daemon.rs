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
    fs::{File, remove_file},
    os::fd::{AsFd, AsRawFd},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, unix::AsyncFd},
    net::{UnixListener, UnixStream},
};
use tracing::{error, info, instrument};

use crate::error::{Error, Result};
use remux_core::{
    daemon_utils::{get_sock_path, lock_daemon_file},
    messages::{self, RemuxDaemonRequest},
};

#[derive(Debug)]
pub struct RemuxDaemon {
    _daemon_file: File, // daemon must hold the exclusive file lock while it is alive and running
}

impl RemuxDaemon {
    /// Makes sure there can only ever be once instance at the
    /// process level through use of OS level file locks
    pub fn new() -> Result<Self> {
        Ok(Self {
            _daemon_file: lock_daemon_file()?,
        })
    }

    #[instrument(skip(self))]
    pub async fn listen(&self) -> Result<()> {
        let socket_path = get_sock_path()?;

        if socket_path.exists() {
            remove_file(&socket_path)?;
        }

        info!("unix socket path: {:?}", socket_path);
        let listener = UnixListener::bind(socket_path)?;
        loop {
            let (stream, addr) = listener.accept().await?;
            info!("accepting connection from: {:?}", addr.as_pathname());
            tokio::spawn(async move {
                if let Err(e) = handle_communication(stream).await {
                    error!("{e}");
                }
            });
        }
    }
}

#[instrument]
async fn handle_communication(mut stream: UnixStream) -> Result<()> {
    loop {
        let message: RemuxDaemonRequest = messages::read_message(&mut stream).await?;
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
async fn run_pty(mut stream: UnixStream) -> Result<()> {
    use tokio::sync::mpsc::unbounded_channel;
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
            let (send_to_pty, mut recv_for_pty) = unbounded_channel::<Vec<u8>>();
            let (send_to_tcp, mut recv_for_tcp) = unbounded_channel::<Vec<u8>>();

            let fd = master.as_raw_fd();
            let flags = unsafe { fcntl(fd, F_GETFL) };
            if flags < 0 {
                return Err(Error::Custom("flag error".into()));
            }
            let res = unsafe { fcntl(fd, F_SETFL, flags | O_NONBLOCK) };
            if res < 0 {
                return Err(Error::Custom("fcntl error".into()));
            }

            let pty_fd_task: tokio::task::JoinHandle<std::result::Result<_, Error>> = tokio::spawn(
                async move {
                    let async_fd = AsyncFd::new(master)?;
                    loop {
                        tokio::select! {
                            // read from PTY
                            Ok(mut guard) = async_fd.readable() => {
                                let mut buf = [0u8; 1024];
                                match guard.try_io(|fd| unistd::read(fd.get_ref(), &mut buf).map_err(|e| e.into())) {
                                    Ok(Ok(n)) if n > 0 => {
                                        send_to_tcp.send(buf[..n].to_vec()).map_err(|_e| Error::Custom("couldn't send to tcp".into()))?;
                                    },
                                    Ok(Ok(_)) => {
                                        info!("stopping pty task");
                                        break; // exit out of the loop - fd is closed
                                    },
                                    Ok(Err(e)) => {
                                        error!("Error reading: {e}");
                                    },
                                    Err(_would_block) => {
                                        continue;},
                                }
                            },
                            // write to PTY
                            data_opt = recv_for_pty.recv() => {
                                match data_opt {
                                    Some(data) => {
                                        let mut guard = async_fd.writable().await?;
                                        let _res = guard.try_io(|fd| {
                                            match unistd::write(fd.get_ref(), &data) {
                                                Ok(n) if n > 0 => info!("wrote {n} bytes to pty"),
                                                Ok(_) => info!("wrote 0 bytes to pty"),
                                                Err(e) => error!("error writing to pty: {}", e),
                                            };
                                            Ok(())
                                        }).map_err(|_e| Error::Custom("failed to write to master fd".to_owned()))?;
                                    },
                                    None => {
                                        // None means sender closed the channel (via RAII from tcp_task ending)
                                        info!("stopping pty task");
                                        break;
                                    },
                                }
                            },
                        }
                    }
                    Ok(())
                },
            );

            let tcp_task: tokio::task::JoinHandle<std::result::Result<_, Error>> = tokio::spawn(
                async move {
                    let mut buf = [0u8; 1024];
                    loop {
                        tokio::select! {
                            Ok(n) = stream.read(&mut buf) => {
                                if n > 0 {
                                    send_to_pty.send(buf[..n].to_vec()).map_err(|_e| Error::Custom("could not send to pty".to_owned()))?;
                                } else {
                                    info!("Tcp client disconnected");
                                    break;
                                }
                            },
                            //
                            data_opt = recv_for_tcp.recv() => {
                                match data_opt{
                                    Some(data) => {
                                        info!("writing to tcp: {}", String::from_utf8(data.clone()).map_err(|_| Error::Custom("coudln't convert tcp data to utf8 string".to_owned()))?);
                                        stream.write_all(&data).await?;
                                    },
                                    None => {
                                        // None means sender closed the channel (via RAII from pty_fd_task ending)
                                        info!("stopping tcp task");
                                        break;
                                    },
                                }
                            },
                        }
                    }
                    Ok(())
                },
            );
            let _ = tokio::try_join!(pty_fd_task, tcp_task)?;
            info!("pty ran to completion");
            Ok(())
        }
        Child => {
            let cmd = CString::new("/bin/zsh")
                .map_err(|_| Error::Custom("couldn't spawn shell process in PTY".to_owned()))?;
            execvp(&cmd, std::slice::from_ref(&cmd))?;
            unreachable!();
        }
    }
}

#[cfg(test)]
mod test {
    #[tokio::test]
    async fn name() {}
}

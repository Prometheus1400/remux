use std::{
    ffi::CString,
    os::fd::{AsFd, AsRawFd},
};

use nix::{
    libc::{F_GETFL, F_SETFL, O_NONBLOCK, fcntl},
    pty::{
        ForkptyResult::{Child, Parent},
        forkpty,
    },
    unistd::{self, execvp},
};
use tokio::{
    io::unix::AsyncFd,
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
    task::JoinHandle,
};
use tracing::{error, info};

use crate::error::{Error, Result};

type Task = JoinHandle<std::result::Result<(), Error>>;

pub struct PtyProcessBuilder {
    pty_tx: UnboundedSender<u8>,
    pty_rx: UnboundedReceiver<u8>,
    output_tx: UnboundedSender<u8>,
    exit_callbacks: Vec<Box<dyn FnOnce() + Send + 'static>>,
}

#[allow(unused)]
impl PtyProcessBuilder {
    pub fn new(output_tx: UnboundedSender<u8>) -> Self {
        let (pty_tx, pty_rx) = mpsc::unbounded_channel::<u8>();
        Self {
            pty_tx,
            pty_rx,
            output_tx,
            exit_callbacks: vec![],
        }
    }

    pub fn with_exit_callback<F>(mut self, callback: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        self.exit_callbacks.push(Box::new(callback));
        self
    }

    pub fn build(mut self) -> Result<PtyProcesss> {
        let fork_result = unsafe { forkpty(None, None)? };
        match fork_result {
            // child just goes off on its own and runs the shell
            Child => {
                let cmd = CString::new("/bin/zsh")
                    .map_err(|_| Error::Custom("couldn't spawn shell process in PTY".to_owned()))?;
                execvp(&cmd, std::slice::from_ref(&cmd))?;
                unreachable!();
            }
            // one tokio task that just wraps the master FD
            // one task that just wraps the stream
            // they can communicate via channels !!
            Parent { child, master } => {
                info!("parent PID: {}", master.as_fd().as_raw_fd());
                info!("child PID: {}", child.as_raw());
                // let (send_to_pty, mut recv_for_pty) = unbounded_channel::<u8>();
                // let (send_to_history, mut recv_for_history) = unbounded_channel::<String>(); // sender should send lines

                let fd = master.as_raw_fd();
                let flags = unsafe { fcntl(fd, F_GETFL) };
                if flags < 0 {
                    return Err(Error::Custom("flag error".into()));
                }
                let res = unsafe { fcntl(fd, F_SETFL, flags | O_NONBLOCK) };
                if res < 0 {
                    return Err(Error::Custom("fcntl error".into()));
                }

                let pty_task: Task = tokio::spawn(async move {
                    let async_fd = AsyncFd::new(master)?;
                    loop {
                        tokio::select! {
                            // read from PTY
                            Ok(mut guard) = async_fd.readable() => {
                                let mut buf = [0u8; 1024];
                                match guard.try_io(|fd| unistd::read(fd.get_ref(), &mut buf).map_err(|e| e.into())) {
                                    Ok(Ok(n)) if n > 0 => {
                                        for byte in &buf[..n] {
                                            self.output_tx.send(*byte).map_err(|_e| Error::Custom("couldn't send to output".into()))?;
                                        }
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
                            data_opt = self.pty_rx.recv() => {
                                match data_opt {
                                    Some(data) => {
                                        let mut guard = async_fd.writable().await?;
                                        let _res = guard.try_io(|fd| {
                                            match unistd::write(fd.get_ref(), &[data]) {
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
                    // TODO: need to send some sort of event that eventually triggers the
                    // corresponding pane to get killed too
                    // can probably use these callbacks
                    for callback in self.exit_callbacks {
                        callback();
                    }
                    Ok(())
                });

                Ok(PtyProcesss {
                    pty_tx: self.pty_tx,
                    pty_task,
                })
            }
        }
    }
}

pub struct PtyProcesss {
    // channels for sending to pty process -> sends into child process
    pty_tx: UnboundedSender<u8>,
    // tokyo tasks
    pty_task: Task,
}

#[allow(unused)]
impl PtyProcesss {
    pub fn is_running(&self) -> bool {
        !self.pty_task.is_finished()
    }

    pub fn send_byte(&mut self, byte: u8) -> Result<()> {
        self.pty_tx
            .send(byte)
            .map_err(|_| Error::Custom("error sending byte to pty process".to_owned()))?;
        Ok(())
    }

    pub fn kill(self) {
        todo!();
    }
}

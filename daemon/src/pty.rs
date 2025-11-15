use std::{
    ffi::CString,
    os::fd::{AsFd, AsRawFd},
};

use bytes::Bytes;
use nix::{
    errno::Errno,
    libc::{F_GETFL, F_SETFL, O_NONBLOCK, fcntl},
    pty::{
        ForkptyResult::{Child, Parent},
        forkpty,
    },
    sys::{
        signal::{Signal, kill},
        wait::{WaitStatus, waitpid},
    },
    unistd::{self, execvp},
};
use tokio::{
    io::unix::AsyncFd,
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
    task::JoinHandle,
};
use tracing::{debug, error, info};

use crate::error::{Error, Result};

type Task = JoinHandle<std::result::Result<(), Error>>;

pub struct PtyProcessBuilder {
    pty_tx: UnboundedSender<Bytes>,
    pty_rx: UnboundedReceiver<Bytes>,
    output_tx: UnboundedSender<Bytes>,
    exit_callbacks: Vec<Box<dyn FnOnce() + Send + 'static>>,
}

#[allow(unused)]
impl PtyProcessBuilder {
    pub fn new(output_tx: UnboundedSender<Bytes>) -> Self {
        let (pty_tx, pty_rx) = mpsc::unbounded_channel::<Bytes>();
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
                                        self.output_tx.send(
                                            Bytes::copy_from_slice(&buf[..n]))
                                            .map_err(|_e| Error::Custom("couldn't send to output".into())
                                        )?;
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
                                            match unistd::write(fd.get_ref(), &data) {
                                                Ok(n) if n > 0 => debug!("wrote {n} bytes to pty"),
                                                Ok(_) => debug!("wrote 0 bytes to pty"),
                                                Err(e) => error!("error writing to pty: {}", e),
                                            };
                                            Ok(())
                                        }).map_err(|_e| Error::Custom("failed to write to master fd".to_owned()))?;
                                    },
                                    None => {
                                        // None means sender closed the channel - and we need to
                                        // clean up the child process
                                        kill(child, Signal::SIGKILL)?;
                                        info!("killing pty child process {child}");
                                        break;
                                    },
                                }
                            },
                        }
                    }
                    match waitpid(child, None) {
                        Ok(status) => match status {
                            WaitStatus::Exited(child, code) => {
                                info!("Process {} exited with code {}", child, code);
                            }
                            WaitStatus::Signaled(child, signal, _) => {
                                info!("Process {} killed by signal {:?}", child, signal);
                            }
                            _ => {
                                info!("Process {:?} changed state: {:?}", child, status);
                            }
                        },
                        Err(Errno::ECHILD) => info!("No such child process: {}", child),
                        Err(err) => error!("waitpid failed: {}", err),
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
    pty_tx: UnboundedSender<Bytes>,
    // tokyo tasks
    pty_task: Task,
}

#[allow(unused)]
impl PtyProcesss {
    pub fn is_running(&self) -> bool {
        !self.pty_task.is_finished()
    }

    pub fn send_bytes(&mut self, bytes: Bytes) -> Result<()> {
        self.pty_tx
            .send(bytes)
            .map_err(|_| Error::Custom("error sending byte to pty process".to_owned()))?;
        Ok(())
    }

    pub fn get_sender(&self) -> &UnboundedSender<Bytes> {
        &self.pty_tx
    }

    pub fn kill(self) {
        todo!();
    }
}

use std::{ffi::CString, os::fd::AsRawFd};

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
    sync::{mpsc, watch},
    task::JoinHandle,
};
use tracing::{debug, error, info};

use crate::{error::{Error, Result}, types::NoResTask};

pub struct PtyProcessBuilder {
    pty_tx: mpsc::UnboundedSender<Bytes>,
    pty_rx: mpsc::UnboundedReceiver<Bytes>,
    output_tx: mpsc::UnboundedSender<Bytes>,
    closed_tx: watch::Sender<bool>,
    exit_callbacks: Vec<Box<dyn FnOnce() + Send + 'static>>,
}

#[allow(unused)]
impl PtyProcessBuilder {
    pub fn new(output_tx: mpsc::UnboundedSender<Bytes>, closed_tx: watch::Sender<bool>) -> Self {
        let (pty_tx, pty_rx) = mpsc::unbounded_channel::<Bytes>();
        Self {
            pty_tx,
            pty_rx,
            output_tx,
            closed_tx,
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
        debug!("forking and spawning child PTY process");
        let fork_result = unsafe { forkpty(None, None)? };
        match fork_result {
            // child just goes off on its own and runs the shell
            Child => {
                let cmd = CString::new("/bin/zsh")
                    .map_err(|_| Error::Custom("couldn't spawn shell process in PTY".to_owned()))?;
                execvp(&cmd, std::slice::from_ref(&cmd))?;
                unreachable!();
            }
            Parent { child, master } => {
                debug!("child PID: {}", child.as_raw());
                let fd = master.as_raw_fd();
                let flags = unsafe { fcntl(fd, F_GETFL) };
                if flags < 0 {
                    return Err(Error::Custom("flag error".into()));
                }
                let res = unsafe { fcntl(fd, F_SETFL, flags | O_NONBLOCK) };
                if res < 0 {
                    return Err(Error::Custom("fcntl error".into()));
                }

                let pty_task: NoResTask = tokio::spawn(async move {
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
                    self.closed_tx.send(true);
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
    pty_tx: mpsc::UnboundedSender<Bytes>,
    // tokyo tasks
    pty_task: NoResTask,
}

#[allow(unused)]
impl PtyProcesss {
    pub fn is_running(&self) -> bool {
        !self.pty_task.is_finished()
    }

    pub fn get_sender(&self) -> &mpsc::UnboundedSender<Bytes> {
        &self.pty_tx
    }

    pub fn kill(self) {
        todo!();
    }
}

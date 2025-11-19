use std::{
    ffi::CString,
    os::fd::{AsRawFd, OwnedFd},
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
    unistd::{self, Pid, execvp},
};
use tokio::{io::unix::AsyncFd, sync::mpsc};
use tracing::{debug, error, info, trace};

use crate::{
    actor::{Actor, ActorHandle}, error::{Error, Result}, pane::PaneHandle, types::DaemonTask
};

#[derive(Debug, Clone)]
pub enum PtyEvent {
    Kill,
    Input(Bytes),
}

pub struct PtyBuilder {
    pane_handle: PaneHandle,
    exit_callbacks: Vec<Box<dyn FnOnce() + Send + Sync + 'static>>,
}

#[allow(unused)]
impl PtyBuilder {
    pub fn new(pane_handle: PaneHandle) -> Self {
        Self {
            pane_handle,
            exit_callbacks: vec![],
        }
    }

    pub fn with_exit_callback<F>(mut self, callback: F) -> Self
    where
        F: FnOnce() + Send + Sync + 'static,
    {
        self.exit_callbacks.push(Box::new(callback));
        self
    }

    pub fn build(self) -> Pty {
        Pty::new(self)
    }
}

pub struct Pty {
    // used for sending events to the actor
    tx: mpsc::Sender<PtyEvent>,
    rx: mpsc::Receiver<PtyEvent>,
    // channels for sending to pty process -> sends into child process
    pty_tx: mpsc::UnboundedSender<Bytes>,
    pty_rx: mpsc::UnboundedReceiver<Bytes>,
    pane_handle: PaneHandle,
    exit_callbacks: Vec<Box<dyn FnOnce() + Send + Sync + 'static>>,
}
impl Pty {
    fn new(builder: PtyBuilder) -> Self {
        let (tx, rx) = mpsc::channel::<PtyEvent>(10);
        let (pty_tx, pty_rx) = mpsc::unbounded_channel::<Bytes>();
        Self {
            tx,
            rx,
            pty_tx,
            pty_rx,
            pane_handle: builder.pane_handle,
            exit_callbacks: builder.exit_callbacks,
        }
    }

    fn handle_input(&mut self, bytes: Bytes) -> Result<()> {
        self.pty_tx
            .send(bytes)
            .map_err(|_| Error::Custom("error sending to pty_tx".to_owned()))
    }

    fn handle_kill(child: Pid) -> Result<()> {
        kill(child, Signal::SIGKILL)?;
        info!("killing pty child process {child}");
        Ok(())
    }
}
impl Actor<PtyHandle> for Pty {
    fn run(mut self) -> Result<PtyHandle> {
        debug!("forking and spawning child PTY process");
        let fork_result = unsafe { forkpty(None, None)? };

        match fork_result {
            // child just goes off on its own and runs the shell
            Child => run_child(),
            Parent { child, master } => {
                debug!("child PID: {}", child.as_raw());
                set_fd_nonblocking(&master)?;
                let handle = PtyHandle::new(self.tx.clone());

                let async_fd = AsyncFd::new(master)?;
                let _task: DaemonTask = tokio::spawn({
                    let handler = handle.clone();
                    async move {
                        loop {
                            tokio::select! {
                                // read from PTY
                                Ok(mut guard) = async_fd.readable() => {
                                    let mut buf = [0u8; 1024];
                                    match guard.try_io(|fd| unistd::read(fd.get_ref(), &mut buf).map_err(|e| e.into())) {
                                        Ok(Ok(n)) if n > 0 => {
                                            self.pane_handle.send_output_from_pty(Bytes::copy_from_slice(&buf[..n])).await.unwrap()
                                        },
                                        Ok(Ok(_)) => {
                                            handler.kill().await?;
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
                                                    Ok(n) if n > 0 => trace!("wrote {n} bytes to pty"),
                                                    Ok(_) => trace!("wrote 0 bytes to pty"),
                                                    Err(e) => error!("error writing to pty: {}", e),
                                                };
                                                Ok(())
                                            }).map_err(|_e| Error::Custom("failed to write to master fd".to_owned()))?;
                                        },
                                        None => {
                                            // None means sender closed the channel - and we need to
                                            // clean up the child process
                                            handler.kill().await?;
                                            break;
                                        },
                                    }
                                },
                                // event handler
                                Some(event) = self.rx.recv() => {
                                    let res = match event.clone() {
                                        PtyEvent::Kill => {
                                            Self::handle_kill(child)?;
                                            break;
                                        }
                                        PtyEvent::Input(bytes) => self.handle_input(bytes.clone()),
                                    };
                                    if let Err(e) = res {
                                        error!("error handling {event:?} in PtyProcess: {e}");
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
                        for callback in self.exit_callbacks.drain(..) {
                            callback();
                        }
                        debug!("stopping PtyProcess run");
                        self.pane_handle.kill().await.unwrap();
                        Ok(())
                    }
                });

                Ok(handle)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct PtyHandle {
    tx: mpsc::Sender<PtyEvent>,
}
impl ActorHandle for PtyHandle {
    async fn kill(&self) -> Result<()> {
        self.tx
            .send(PtyEvent::Kill)
            .await
            .map_err(|_| Error::Custom("error sending kill to pty process".to_owned()))
    }
    fn is_alive(&self) -> bool {
        !self.tx.is_closed()
    }
}
impl PtyHandle {
    fn new(tx: mpsc::Sender<PtyEvent>) -> Self {
        Self { tx }
    }

    pub async fn send(&self, bytes: Bytes) -> Result<()> {
        self.tx
            .send(PtyEvent::Input(bytes))
            .await
            .map_err(|_| Error::Custom("error sending input to pty process".to_owned()))
    }
}

fn set_fd_nonblocking(owned_fd: &OwnedFd) -> Result<()> {
    let fd = owned_fd.as_raw_fd();
    let flags = unsafe { fcntl(fd, F_GETFL) };
    if flags < 0 {
        return Err(Error::Custom("flag error".into()));
    }
    let res = unsafe { fcntl(fd, F_SETFL, flags | O_NONBLOCK) };
    if res < 0 {
        Err(Error::Custom("fcntl error".into()))
    } else {
        Ok(())
    }
}

fn run_child() -> ! {
    let cmd = CString::new("/bin/zsh").expect("couldn't spawn shell process in PTY");
    let _ = execvp(&cmd, std::slice::from_ref(&cmd));
    eprintln!("failed to exec shell");
    std::process::exit(1);
}

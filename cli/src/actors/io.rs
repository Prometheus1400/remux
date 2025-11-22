use std::io;

use bytes::Bytes;
use remux_core::{
    communication,
    events::{CliEvent, DaemonEvent},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    sync::mpsc,
};
use tracing::debug;

use crate::{
    actors::popup::{Popup, PopupHandle},
    prelude::*,
};

#[derive(Debug)]
pub enum IOEvent {
    RawInput { bytes: Bytes },
    SwitchSession { session_id: Option<u32>},
}
use IOEvent::*;

#[derive(Debug)]
enum IOState {
    Blocked,
    Unblocked,
}

#[derive(Debug)]
pub struct IO {
    handle: IOHandle,
    rx: mpsc::Receiver<IOEvent>,
    stream: UnixStream,
    popup_handle: PopupHandle,
    state: IOState,
}
impl IO {
    pub fn spawn(stream: UnixStream) -> Result<CliTask> {
        IO::new(stream)?.run()
    }

    fn new(stream: UnixStream) -> Result<Self> {
        let (tx, rx) = mpsc::channel(100000);
        let handle = IOHandle { tx };
        let popup_handle = Popup::spawn(handle.clone())?;
        Ok(Self {
            handle,
            rx,
            stream,
            popup_handle,
            state: IOState::Unblocked,
        })
    }

    fn run(mut self) -> Result<CliTask> {
        let task: CliTask = tokio::spawn({
            let mut stdin = tokio::io::stdin(); // read here
            let mut stdin_buf = [0u8; 1024];
            let mut stdout = tokio::io::stdout(); // read here
            async move {
                loop {
                    tokio::select! {
                        Some(event) = self.rx.recv() => {
                            match event {
                                RawInput {bytes} => {
                                    debug!("IO: IO Event(RawInput)");
                                    communication::send_event(&mut self.stream, CliEvent::Raw { bytes: bytes.into() }).await.unwrap();
                                }
                                SwitchSession {session_id} => {
                                    debug!("IO: IO Event(SwitchSession)");
                                    if let Some(session_id) = session_id {
                                        communication::send_event(&mut self.stream, CliEvent::SwitchSession { session_id }).await.unwrap();
                                    }
                                    self.state = IOState::Unblocked;
                                }
                            }
                        },
                        res = communication::recv_daemon_event(&mut self.stream), if matches!(self.state, IOState::Unblocked) => {
                            match res {
                                Ok(event) => {
                                    match event {
                                        DaemonEvent::Raw{bytes} => {
                                            debug!("IO: DaemonEvent(Raw)");
                                            stdout.write_all(&bytes).await.unwrap();
                                            stdout.flush().await.unwrap();
                                        }
                                        DaemonEvent::SwitchSessionOptions{session_ids} => {
                                            debug!("IO: DaemonEvent(SwitchSessionOptions)");
                                            self.state = IOState::Blocked;
                                            self.popup_handle.send_select_session(session_ids).await.unwrap();
                                            // todo!();
                                        }
                                    }
                                }
                                Err(_) => {
                                    break;
                                }
                            }
                        }
                        stdin_res = stdin.read(&mut stdin_buf), if matches!(self.state, IOState::Unblocked) => {
                            match stdin_res {
                                Ok(n) if n > 0 => {
                                    self.handle.send_raw_input(Bytes::copy_from_slice(&stdin_buf[..n])).await.unwrap()
                                }
                                Ok(_) => {
                                    break;
                                }
                                Err(_) => {
                                    continue;
                                }
                            }
                        },
                    }
                }
                debug!("Gateway stopped");
            }
        });

        Ok(task)
    }
}

#[derive(Debug, Clone)]
pub struct IOHandle {
    tx: mpsc::Sender<IOEvent>,
}
impl IOHandle {
    pub async fn send_raw_input(&mut self, bytes: Bytes) -> Result<()> {
        Ok(self.tx.send(IOEvent::RawInput { bytes }).await?)
    }
    pub async fn send_switch_session(&mut self, session_id: Option<u32>) -> Result<()> {
        Ok(self.tx.send(IOEvent::SwitchSession { session_id }).await?)
    }
}

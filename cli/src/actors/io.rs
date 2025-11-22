use std::io;

use bytes::Bytes;
use remux_core::{
    communication,
    events::{CliEvent, DaemonEvent},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    sync::mpsc::{self, Sender},
};
use tracing::debug;

use crate::{
    actors::popup::{Popup, PopupHandle},
    prelude::*,
    widgets,
};

#[derive(Debug)]
pub enum ClientEvent {
    // RawInput { bytes: Bytes },
    SwitchSession { session_id: Option<u32> },
}
use ClientEvent::*;

#[derive(Debug)]
enum StdinState {
    Daemon,
    Popup,
}

#[derive(Debug)]
enum DaemonEventsState {
    Blocked,
    Unblocked,
}

#[derive(Debug)]
pub struct Client {
    handle: IOHandle,
    rx: mpsc::Receiver<ClientEvent>,
    stream: UnixStream,
    popup_handle: PopupHandle,
    stdin_state: StdinState,
    daemon_events_state: DaemonEventsState,

    // for stdin routing
    // daemon_tx: mpsc::Sender<Bytes>,
    // daemon_rx: mpsc::Receiver<Bytes>,
    popup_tx: mpsc::Sender<Bytes>,
}
impl Client {
    pub fn spawn(stream: UnixStream) -> Result<CliTask> {
        Client::new(stream)?.run()
    }

    fn new(stream: UnixStream) -> Result<Self> {
        let (tx, rx) = mpsc::channel(100);
        // let (daemon_tx, daemon_rx) = mpsc::channel(100);
        let (popup_tx, popup_rx) = mpsc::channel(100);
        let handle = IOHandle { tx };
        let popup_handle = Popup::spawn(popup_rx, handle.clone())?;
        Ok(Self {
            handle,
            rx,
            stream,
            popup_handle,
            stdin_state: StdinState::Daemon,
            daemon_events_state: DaemonEventsState::Unblocked,
            // daemon_tx,
            // daemon_rx,
            popup_tx,
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
                                // RawInput {bytes} => {
                                //     match self.stdin_state {
                                //         StdinState::Daemon => {
                                //             debug!("IO: IO Event(RawInput) - sending to daemon");
                                //             communication::send_event(&mut self.stream, CliEvent::Raw { bytes: bytes.into() }).await.unwrap();
                                //         }
                                //         StdinState::Popup => {
                                //             debug!("IO: IO Event(RawInput) - sending to popup");
                                //             self.popup_handle.send_input(bytes).await.unwrap();
                                //         }
                                //     }
                                // }
                                SwitchSession {session_id} => {
                                    debug!("IO: IO Event(SwitchSession)");
                                    if let Some(session_id) = session_id {
                                        communication::send_event(&mut self.stream, CliEvent::SwitchSession { session_id }).await.unwrap();
                                    }
                                    self.daemon_events_state = DaemonEventsState::Unblocked;
                                    self.stdin_state = StdinState::Daemon;
                                }
                            }
                        },
                        res = communication::recv_daemon_event(&mut self.stream), if matches!(self.daemon_events_state, DaemonEventsState::Unblocked) => {
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
                                            self.daemon_events_state = DaemonEventsState::Blocked;
                                            self.stdin_state = StdinState::Popup;
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
                        stdin_res = stdin.read(&mut stdin_buf) => {
                            match stdin_res {
                                Ok(n) if n > 0 => {
                                    match self.stdin_state {
                                        StdinState::Daemon => {
                                            // self.handle.send_raw_input(Bytes::copy_from_slice(&stdin_buf[..n])).await.unwrap();
                                            communication::send_event(&mut self.stream, CliEvent::Raw{bytes: stdin_buf[..n].to_vec() }).await.unwrap();
                                            
                                        },
                                        StdinState::Popup => {
                                            self.popup_tx.send(Bytes::copy_from_slice(&stdin_buf[..n])).await.unwrap();
                                        },
                                    }
                                    // self.handle.send_raw_input(Bytes::copy_from_slice(&stdin_buf[..n])).await.unwrap()
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
    tx: mpsc::Sender<ClientEvent>,
}
impl IOHandle {
    // pub async fn send_raw_input(&mut self, bytes: Bytes) -> Result<()> {
    //     Ok(self.tx.send(ClientEvent::RawInput { bytes }).await?)
    // }
    pub async fn send_switch_session(&mut self, session_id: Option<u32>) -> Result<()> {
        Ok(self
            .tx
            .send(ClientEvent::SwitchSession { session_id })
            .await?)
    }
}

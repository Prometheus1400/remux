use bytes::Bytes;
use handle_macro::Handle;
use remux_core::{comm, events::DaemonEvent};
use tokio::{net::UnixStream, sync::mpsc};
use tracing::Instrument;

use crate::{actors::session_manager::SessionManagerHandle, layout::SplitDirection, prelude::*};

#[allow(unused)]
#[derive(Handle)]
pub enum ClientConnectionEvent {
    AttachToSession(u32),
    SuccessAttachToSession(u32),
    FailedAttachToSession(u32),
    DetachFromSession(u32),
    SessionOutput(Bytes),
    NewSession(u32),
    CurrentSessions(Vec<u32>),
    Disconnect,
}
use ClientConnectionEvent::*;

#[allow(unused)]
enum ClientConnectionState {
    Unattached,
    Attaching(u32),
    Attached(u32),
}

pub struct ClientConnection {
    id: u32,
    stream: UnixStream,
    handle: ClientConnectionHandle,
    rx: mpsc::Receiver<ClientConnectionEvent>,
    session_manager_handle: SessionManagerHandle,
    state: ClientConnectionState,
}
impl ClientConnection {
    #[instrument(skip(stream, session_manager_handle))]
    pub fn spawn(stream: UnixStream, session_manager_handle: SessionManagerHandle) -> Result<ClientConnectionHandle> {
        let client = Self::new(stream, session_manager_handle);
        client.run()
    }
    #[instrument(skip(stream, session_manager_handle))]
    fn new(stream: UnixStream, session_manager_handle: SessionManagerHandle) -> Self {
        let (tx, rx) = mpsc::channel(10);
        let handle = ClientConnectionHandle { tx };
        let id: u32 = rand::random();
        Self {
            id,
            stream,
            handle,
            rx,
            session_manager_handle,
            state: ClientConnectionState::Unattached,
        }
    }
    #[instrument(skip(self), fields(client_id = self.id))]
    fn run(mut self) -> crate::error::Result<ClientConnectionHandle> {
        let span = tracing::Span::current();
        let handle_clone = self.handle.clone();
        let _task = tokio::spawn({
            async move {
                let handle = self.handle.clone();
                loop {
                    use remux_core::events::CliEvent;
                    tokio::select! {
                        Some(event) = self.rx.recv() => {
                            match event {
                                AttachToSession(session_id) => {
                                    debug!("Client: AttachToSession");
                                    self.session_manager_handle.client_connect(self.id, handle.clone(), session_id, true).await.unwrap();
                                    self.state = ClientConnectionState::Attaching(session_id);
                                }
                                SuccessAttachToSession(session_id) => {
                                    debug!("Client: SuccessAttachToSession");
                                    self.state = ClientConnectionState::Attached(session_id);
                                    comm::send_event(&mut self.stream, DaemonEvent::ActiveSession(session_id)).await.unwrap();
                                }
                                FailedAttachToSession{..} => {
                                    debug!("Client: FailedAttachToSession");
                                    comm::send_event(&mut self.stream, DaemonEvent::Disconnected).await.unwrap();
                                }
                                DetachFromSession{..} => {
                                    debug!("Client: DetachFromSession");
                                    self.state = ClientConnectionState::Unattached;
                                }
                                Disconnect => {
                                    trace!("Client: Disconnect");
                                    comm::send_event(&mut self.stream, DaemonEvent::Disconnected).await.unwrap();
                                }
                                SessionOutput(bytes) => {
                                    trace!("Client: SessionOutput");
                                    comm::send_event(&mut self.stream, DaemonEvent::Raw(bytes)).await.unwrap();
                                }
                                NewSession(session_id) => {
                                    trace!("Client: NewSession");
                                    comm::send_event(&mut self.stream, DaemonEvent::NewSession(session_id)).await.unwrap();
                                }
                                CurrentSessions(session_ids) => {
                                    trace!("Client: NewSession");
                                    comm::send_event(&mut self.stream, DaemonEvent::CurrentSessions(session_ids)).await.unwrap();
                                }
                            }
                        },
                        res = comm::recv_cli_event(&mut self.stream), if matches!(self.state, ClientConnectionState::Attached(_)) => {
                            match res {
                                Ok(event) => {
                                    match event {
                                        CliEvent::Raw(bytes) => {
                                            trace!("Client Event Input: raw({bytes:?})");
                                            self.session_manager_handle.user_input(self.id, bytes).await.unwrap();
                                        },
                                        CliEvent::Detach => {
                                            trace!("Client Event Input: detach");
                                            self.session_manager_handle.client_disconnect(self.id).await.unwrap();
                                        },
                                        CliEvent::KillPane => {
                                            debug!("Client Event Input: kill pane");
                                            self.session_manager_handle.user_kill_pane(self.id).await.unwrap();
                                        },
                                        CliEvent::SplitPaneHorizontal => {
                                            debug!("Client Event Input: horizontal split pane");
                                            self.session_manager_handle.user_split_pane(self.id, SplitDirection::Horizontal).await.unwrap();
                                        },
                                        CliEvent::SplitPaneVertical => {
                                            debug!("Client Event Input: vertical split pane");
                                            self.session_manager_handle.user_split_pane(self.id, SplitDirection::Vertical).await.unwrap();
                                        },
                                        CliEvent::NextPane => {
                                            debug!("Client Event Input: next pane");
                                            self.session_manager_handle.user_iterate_pane(self.id, true).await.unwrap();
                                        },
                                        CliEvent::PrevPane => {
                                            debug!("Client Event Input: prev pane");
                                            self.session_manager_handle.user_iterate_pane(self.id, false).await.unwrap();
                                        },
                                        CliEvent::SwitchSession(session_id) => {
                                            debug!("Client Event Input: SwitchSession{session_id}");
                                            self.session_manager_handle.client_switch_session(self.id, session_id).await.unwrap();
                                        }
                                    }
                                }
                                Err(e) => {
                                    // client disconnected
                                    debug!("Client disconnected because of error recieving cli event: {e}");
                                    self.session_manager_handle.client_disconnect(self.id).await.unwrap();
                                    break;
                                }
                            }
                        }
                    }
                }
            }.instrument(span)
        });

        Ok(handle_clone)
    }
}

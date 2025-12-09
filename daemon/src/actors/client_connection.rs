use bytes::Bytes;
use handle_macro::Handle;
use remux_core::{
    comm,
    events::DaemonEvent,
    messages::{ResponseBuilder, ResponseResult, response},
    states::DaemonState,
};
use tokio::{net::UnixStream, sync::mpsc};
use uuid::Uuid;

use crate::{actors::session_manager::SessionManagerHandle, layout::SplitDirection, prelude::*};

#[allow(unused)]
#[derive(Handle, Debug)]
pub enum ClientConnectionEvent {
    // AttachToSession(u32),
    SuccessAttachToSession(u32),
    FailedAttachToSession(u32),
    DetachFromSession(u32),
    SessionOutput(Bytes),
    Disconnect,

    // client side state update events
    NewSession(u32),

    // variants related to initialization phase
    InitialAttach(u32), // invoked directly by the daemon
    // this variant is unique in that it responds to client by sending a message not an event
    InitialAttachResult(Result<DaemonState>),
}
use ClientConnectionEvent::*;

#[allow(unused)]
#[derive(Debug)]
enum ClientConnectionState {
    Unattached,
    Attached,
}

pub struct ClientConnection {
    id: Uuid,
    stream: UnixStream,
    handle: ClientConnectionHandle,
    rx: mpsc::Receiver<ClientConnectionEvent>,
    session_manager_handle: SessionManagerHandle,
    state: ClientConnectionState,
}
impl ClientConnection {
    pub fn spawn(
        id: Uuid,
        stream: UnixStream,
        session_manager_handle: SessionManagerHandle,
        connecting_session_id: u32,
    ) -> Result<ClientConnectionHandle> {
        let client = Self::new(id, stream, session_manager_handle);
        client.run(connecting_session_id)
    }
    fn new(id: Uuid, stream: UnixStream, session_manager_handle: SessionManagerHandle) -> Self {
        let (tx, rx) = mpsc::channel(10);
        let handle = ClientConnectionHandle { tx };

        Self {
            id,
            stream,
            handle,
            rx,
            session_manager_handle,
            state: ClientConnectionState::Unattached,
        }
    }
    fn run(mut self, initial_session_id: u32) -> Result<ClientConnectionHandle> {
        let handle_clone = self.handle.clone();
        let _task = tokio::spawn(
            async move {
                let handle = self.handle.clone();
                self.session_manager_handle.client_connect(self.id, handle.clone(), initial_session_id, true).await?;
                loop {
                    use remux_core::events::CliEvent;
                    tokio::select! {
                        Some(event) = self.rx.recv() => {
                            let span = error_span!("Recieved Client Connection Event");
                            let _guard = span.enter();
                            match &event {
                                SessionOutput(bytes) => {
                                    trace!(event=?event, num_bytes=bytes.len());
                                }
                                _ => {
                                    info!(event=?event);
                                }
                            }
                            match event {
                                InitialAttachResult(result) if matches!(self.state, ClientConnectionState::Unattached) => {
                                    match result {
                                        Ok(daemon_state) => {
                                            let res = ResponseBuilder::default().result(ResponseResult::Success(response::Attach{initial_daemon_state: daemon_state})).build();
                                            info!(respnse=?res, "Sending response");
                                            comm::send_message(&mut self.stream, &res).await.unwrap();
                                            self.state = ClientConnectionState::Attached;
                                        }
                                        Err(e) => {
                                            comm::send_message(&mut self.stream, &ResponseBuilder::default().result(ResponseResult::Failure::<()>(e.to_string())).build()).await.unwrap();
                                        }
                                    }
                                }
                                SuccessAttachToSession(session_id) => {
                                    self.state = ClientConnectionState::Attached;
                                    comm::send_event(&mut self.stream, DaemonEvent::ActiveSession(session_id)).await.unwrap();
                                }
                                FailedAttachToSession(..) => {
                                    comm::send_event(&mut self.stream, DaemonEvent::Disconnected).await.unwrap();
                                }
                                DetachFromSession(..) => {
                                    self.state = ClientConnectionState::Unattached;
                                }
                                Disconnect => {
                                    comm::send_event(&mut self.stream, DaemonEvent::Disconnected).await.unwrap();
                                }
                                SessionOutput(bytes) => {
                                    comm::send_event(&mut self.stream, DaemonEvent::Raw(bytes)).await.unwrap();
                                }
                                NewSession(session_id) => {
                                    comm::send_event(&mut self.stream, DaemonEvent::NewSession(session_id)).await.unwrap();
                                }
                                _ => {
                                    error!(event=?event, state=?self.state, "Unhandled or invalid event for current state");
                                }
                            }
                        },
                        res = comm::recv_cli_event(&mut self.stream), if matches!(self.state, ClientConnectionState::Attached) => {
                            match res {
                                Ok(event) => {
                                    let span = error_span!("Recieved Cli Event", event=?event);
                                    let _guard = span.enter();
                                    match &event {
                                        CliEvent::Raw(..) => {
                                            trace!(event=?event);
                                        }
                                        _ => {
                                            info!(event=?event);
                                        }
                                    }
                                    match event {
                                        CliEvent::Raw(bytes) => {
                                            self.session_manager_handle.user_input(self.id, bytes).await.unwrap();
                                        },
                                        CliEvent::TerminalResize{rows, cols} => {
                                            self.session_manager_handle.terminal_resize(rows, cols).await.unwrap();
                                        },
                                        CliEvent::Detach => {
                                            self.session_manager_handle.client_disconnect(self.id).await.unwrap();
                                        },
                                        CliEvent::KillPane => {
                                            self.session_manager_handle.user_kill_pane(self.id).await.unwrap();
                                        },
                                        CliEvent::SplitPaneHorizontal => {
                                            self.session_manager_handle.user_split_pane(self.id, SplitDirection::Horizontal).await.unwrap();
                                        },
                                        CliEvent::SplitPaneVertical => {
                                            self.session_manager_handle.user_split_pane(self.id, SplitDirection::Vertical).await.unwrap();
                                        },
                                        CliEvent::NextPane => {
                                            self.session_manager_handle.user_iterate_pane(self.id, true).await.unwrap();
                                        },
                                        CliEvent::PrevPane => {
                                            self.session_manager_handle.user_iterate_pane(self.id, false).await.unwrap();
                                        },
                                        CliEvent::SwitchSession(session_id) => {
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
            Ok::<(), Error>(())
            }.instrument(error_span!(parent: None, "Client Actor", id=?self.id))
        );

        Ok(handle_clone)
    }
}

use bytes::Bytes;
use handle_macro::Handle;
use remux_core::{
    comm,
    events::DaemonEvent,
    messages::{ResponseBuilder, ResponseResult, response},
    states::DaemonState,
};
use tokio::{net::UnixStream, sync::mpsc};
use tracing::Instrument;

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
    id: u32,
    stream: UnixStream,
    handle: ClientConnectionHandle,
    rx: mpsc::Receiver<ClientConnectionEvent>,
    session_manager_handle: SessionManagerHandle,
    state: ClientConnectionState,
}
impl ClientConnection {
    #[instrument(skip(stream, session_manager_handle))]
    pub fn spawn(
        stream: UnixStream,
        session_manager_handle: SessionManagerHandle,
        connecting_session_id: u32,
    ) -> Result<ClientConnectionHandle> {
        let client = Self::new(stream, session_manager_handle);
        client.run(connecting_session_id)
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
    fn run(mut self, initial_session_id: u32) -> crate::error::Result<ClientConnectionHandle> {
        let span = tracing::Span::current();
        let handle_clone = self.handle.clone();

        let _task = tokio::spawn({
            async move {
                let handle = self.handle.clone();
                self.session_manager_handle.client_connect(self.id, handle.clone(), initial_session_id, true).await?;
                loop {
                    use remux_core::events::CliEvent;
                    tokio::select! {
                        Some(event) = self.rx.recv() => {
                            match event {
                                InitialAttachResult(result) if matches!(self.state, ClientConnectionState::Unattached) => {
                                    debug!("Client: InitialAttachResult({result:?})");
                                    match result {
                                        Ok(daemon_state) => {
                                            let res = ResponseBuilder::default().result(ResponseResult::Success(response::Attach{initial_daemon_state: daemon_state})).build();
                                            debug!("Sending response: {res:?}");
                                            comm::send_message(&mut self.stream, &res).await.unwrap();
                                            self.state = ClientConnectionState::Attached;
                                        }
                                        Err(e) => {
                                            comm::send_message(&mut self.stream, &ResponseBuilder::default().result(ResponseResult::Failure::<()>(e.to_string())).build()).await.unwrap();
                                        }
                                    }
                                }
                                // AttachToSession(session_id) => {
                                //     debug!("Client: AttachToSession");
                                //     self.session_manager_handle.client_connect(self.id, handle.clone(), session_id, true).await.unwrap();
                                //     self.state = ClientConnectionState::Attaching;
                                // }
                                SuccessAttachToSession(session_id) => {
                                    debug!("Client: SuccessAttachToSession");
                                    self.state = ClientConnectionState::Attached;
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
                                _ => {
                                    error!("Unhandled or invalid event '{:?}' for current state '{:?}'", event, self.state);
                                    panic!("Unhandled or invalid event '{:?}' for current state '{:?}'", event, self.state);
                                    // TODO: better error handling
                                }
                            }
                        },
                        res = comm::recv_cli_event(&mut self.stream), if matches!(self.state, ClientConnectionState::Attached) => {
                            match res {
                                Ok(event) => {
                                    match event {
                                        CliEvent::Raw(bytes) => {
                                            trace!("Client Event Input: raw({bytes:?})");
                                            self.session_manager_handle.user_input(self.id, bytes).await.unwrap();
                                        },
                                        CliEvent::TerminalResize{rows, cols} => {
                                            debug!("Client Event Input: terminal resize(rows={rows}, cols={cols})");
                                            // todo!()
                                        },
                                        CliEvent::Detach => {
                                            debug!("Client Event Input: detach");
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
            Ok::<(), Error>(())
            }.instrument(span)
        });

        Ok(handle_clone)
    }
}

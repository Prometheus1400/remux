use bytes::Bytes;
use remux_core::{communication, events::DaemonEvent};
use tokio::{io::AsyncWriteExt, net::UnixStream, sync::mpsc};
use tracing::Instrument;

use crate::{
    actors::session_manager::SessionManagerHandle,
    control_signals::CLEAR,
    input_parser::{self, InputParser},
    prelude::*,
};

#[allow(unused)]
pub enum ClientConnectionEvent {
    AttachToSession { session_id: u32 },
    SuccessAttachToSession { session_id: u32 },
    RespondRequestSwitchSession { session_ids: Vec<u32> },
    FailedAttachToSession { session_id: u32 },
    DetachFromSession { session_id: u32 },
    SessionOutput { bytes: Bytes },
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
    input_parser: InputParser,
}
impl ClientConnection {
    #[instrument(skip(stream, session_manager_handle))]
    pub fn spawn(
        stream: UnixStream,
        session_manager_handle: SessionManagerHandle,
    ) -> Result<ClientConnectionHandle> {
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
            input_parser: InputParser::new(),
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
                                AttachToSession{session_id} => {
                                    debug!("Client: AttachToSession");
                                    self.session_manager_handle.connect_client(self.id, handle.clone(), session_id, true).await.unwrap();
                                    self.state = ClientConnectionState::Attaching(session_id);
                                }
                                SuccessAttachToSession{session_id} => {
                                    debug!("Client: SuccessAttachToSession");
                                    self.state = ClientConnectionState::Attached(session_id);
                                }
                                FailedAttachToSession{..} => {
                                    debug!("Client: FailedAttachToSession");
                                    self.state = ClientConnectionState::Unattached;
                                    todo!();
                                }
                                DetachFromSession{..} => {
                                    // for now if a client is detached from the session lets just
                                    // kill it
                                    debug!("Client: DetachFromSession");
                                    self.state = ClientConnectionState::Unattached;
                                    self.stream.write_all(CLEAR).await.unwrap();
                                    // break;
                                }
                                RespondRequestSwitchSession { session_ids } => {
                                    debug!("Client: RespondRequestSwitchSession");
                                    communication::send_event(&mut self.stream, DaemonEvent::SwitchSessionOptions { session_ids }).await.unwrap();
                                }
                                SessionOutput{bytes} => {
                                    trace!("Client: SessionOutput");
                                    communication::send_event(&mut self.stream, DaemonEvent::Raw{bytes: bytes.into()}).await.unwrap();
                                }
                            }
                        },
                        res = communication::recv_cli_event(&mut self.stream), if matches!(self.state, ClientConnectionState::Attached(_)) => {
                            match res {
                                Ok(event) => {
                                    match event {
                                        CliEvent::Raw{bytes} => {
                                            for event in self.input_parser.process(&bytes) {
                                                use input_parser::ParsedEvents;
                                                match event {
                                                    ParsedEvents::Raw(bytes) => {
                                                        trace!("Client Event Input: raw({bytes:?})");
                                                        self.session_manager_handle.client_send_user_input(self.id, bytes).await.unwrap();
                                                    },
                                                    ParsedEvents::KillPane => {
                                                        debug!("Client Event Input: kill pane");
                                                        self.session_manager_handle.client_send_kill_pane(self.id).await.unwrap();
                                                    },
                                                    ParsedEvents::SplitPane => {
                                                        debug!("Client Event Input: split pane");
                                                        self.session_manager_handle.client_send_split_pane(self.id).await.unwrap();
                                                    },
                                                    ParsedEvents::RequestSwitchSession => {
                                                        debug!("Client Event Input: request switch session");
                                                        self.session_manager_handle.client_request_switch_session(self.id).await.unwrap();
                                                    }
                                                }
                                            }
                                        }
                                        CliEvent::SwitchSession {session_id} => {
                                            debug!("Client Event Input: SwitchSession{session_id}");
                                            self.session_manager_handle.client_switch_session(self.id, session_id).await.unwrap();
                                        }
                                    }
                                }
                                Err(_) => {
                                    // client disconnected
                                    debug!("Client disconnected");
                                    self.session_manager_handle.disconnect_client(self.id).await.unwrap();
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

#[derive(Debug, Clone)]
pub struct ClientConnectionHandle {
    tx: mpsc::Sender<ClientConnectionEvent>,
}
#[allow(unused)]
impl ClientConnectionHandle {
    handle_method!(send_output, SessionOutput, bytes: Bytes);
    handle_method!(request_session_attach, AttachToSession, session_id: u32);
    handle_method!(respond_request_switch_session, RespondRequestSwitchSession, session_ids: Vec<u32>);
    handle_method!(notify_attach_failed, FailedAttachToSession, session_id: u32);
    handle_method!(notify_attach_succeeded, SuccessAttachToSession, session_id: u32);
    handle_method!(request_session_detach, DetachFromSession, session_id: u32);

    async fn kill(&self) -> crate::error::Result<()> {
        todo!()
    }

    fn is_alive(&self) -> bool {
        todo!()
    }
}

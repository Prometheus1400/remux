use bytes::Bytes;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    sync::mpsc,
};

use crate::{actors::session_manager::SessionManagerHandle, control_signals::CLEAR, input_parser::{self, InputParser}, prelude::*};

#[allow(unused)]
pub enum ClientEvent {
    AttachToSession(u32),
    SuccessAttachToSession(u32),
    FailedAttachToSession(u32),
    DetachFromSession(u32),
    SessionOutput(Bytes),
}

#[allow(unused)]
enum ClientState {
    Unattached,
    Attaching(u32),
    Attached(u32)
}

pub struct Client {
    id: u32,
    stream: UnixStream,
    handle: ClientHandle,
    rx: mpsc::Receiver<ClientEvent>,
    session_manager_handle: SessionManagerHandle,
    state: ClientState,
    input_parser: InputParser,
}
impl Client {
    pub fn spawn(
        stream: UnixStream,
        session_manager_handle: SessionManagerHandle,
    ) -> Result<ClientHandle> {
        let client = Self::new(stream, session_manager_handle);
        client.run()
    }
    fn new(stream: UnixStream, session_manager_handle: SessionManagerHandle) -> Self {
        let (tx, rx) = mpsc::channel(10);
        let handle = ClientHandle { tx };
        let id: u32 = rand::random();
        Self {
            id,
            stream,
            handle,
            rx,
            session_manager_handle,
            state: ClientState::Unattached,
            input_parser: InputParser::new(),
        }
    }
    fn run(mut self) -> crate::error::Result<ClientHandle> {
        trace!("in client run");
        let handle_clone = self.handle.clone();
        let _task = tokio::spawn({
            async move {
                let handle = self.handle.clone();
                let mut buf = [0u8; 1024];
                loop {
                    tokio::select! {
                        Some(event) = self.rx.recv() => {
                            use ClientEvent::*;
                            match event {
                                AttachToSession(session_id) => {
                                    trace!("Client: AttachToSession");
                                    self.session_manager_handle.connect_client(self.id, handle.clone(), session_id).await.unwrap();
                                    self.state = ClientState::Attaching(session_id);
                                }
                                SuccessAttachToSession(session_id) => {
                                    trace!("Client: SuccessAttachToSession");
                                    self.state = ClientState::Attached(session_id);
                                }
                                FailedAttachToSession(_) => {
                                    trace!("Client: FailedAttachToSession");
                                    self.state = ClientState::Unattached;
                                    todo!();
                                }
                                DetachFromSession(_) => {
                                    // for now if a client is detached from the session lets just
                                    // kill it
                                    trace!("Client: DetachFromSession");
                                    self.state = ClientState::Unattached;
                                    self.stream.write_all(CLEAR).await.unwrap();
                                    break;
                                }
                                SessionOutput(bytes) => {
                                    trace!("Client: SessionOutput");
                                    self.stream.write_all(&bytes).await.unwrap();
                                },
                            }
                        },
                        Ok(n) = self.stream.read(&mut buf), if matches!(self.state, ClientState::Attached(_)) => {
                            match n {
                                0 => {
                                    // client disconnected
                                    self.session_manager_handle.disconnect_client(self.id).await.unwrap();
                                    break;
                                },
                                _ => {
                                    // TODO: need some kind of parsing for control signals
                                    let input = &buf[..n];
                                    for event in self.input_parser.process(input) {
                                        match event {
                                            input_parser::ParsedEvents::Raw(bytes) => {
                                                debug!("Client Event Input: raw({bytes:?})");
                                                self.session_manager_handle.client_send_user_input(self.id, bytes).await.unwrap();
                                            },
                                            input_parser::ParsedEvents::KillPane => {
                                                debug!("Client Event Input: kill pane");
                                                self.session_manager_handle.client_send_kill_pane(self.id).await.unwrap();
                                            },
                                            input_parser::ParsedEvents::SplitPane => {
                                                debug!("Client Event Input: split pane");
                                                self.session_manager_handle.client_send_split_pane(self.id).await.unwrap();
                                            },
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        Ok(handle_clone)
    }

    fn parse_input(&mut self, input: &[u8]) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ClientHandle {
    tx: mpsc::Sender<ClientEvent>,
}
#[allow(unused)]
impl ClientHandle {
    pub async fn send_output(&mut self, bytes: Bytes) -> Result<()> {
        Ok(self.tx.send(ClientEvent::SessionOutput(bytes)).await?)
    }

    pub async fn request_session_attach(&mut self, session_id: u32) -> Result<()> {
        Ok(self
            .tx
            .send(ClientEvent::AttachToSession(session_id))
            .await?)
    }

    pub async fn notify_attach_failed(&mut self, session_id: u32) -> Result<()> {
        Ok(self.tx.send(ClientEvent::FailedAttachToSession(session_id)).await?)
    }

    pub async fn notify_attach_succeeded(&mut self, session_id: u32) -> Result<()> {
        Ok(self.tx.send(ClientEvent::SuccessAttachToSession(session_id)).await?)
    }

    async fn kill(&self) -> crate::error::Result<()> {
        todo!()
    }

    fn is_alive(&self) -> bool {
        todo!()
    }
}

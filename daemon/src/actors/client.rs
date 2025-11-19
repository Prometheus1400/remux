use bytes::Bytes;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    sync::mpsc,
};

use crate::{actors::session_manager::SessionManagerHandle, prelude::*};

pub enum ClientEvent {
    AttachToSession(u32),
    FailedAttachToSession,
    SessionOutput(Bytes),
}

pub struct Client {
    id: u32,
    stream: UnixStream,
    handle: ClientHandle,
    rx: mpsc::Receiver<ClientEvent>,
    session_manager_handle: SessionManagerHandle,
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
                                }
                                SessionOutput(bytes) => {
                                    trace!("Client: SessionOutput");
                                    self.stream.write_all(&bytes).await.unwrap();
                                },
                                FailedAttachToSession => {
                                    trace!("Client: FailedAttachToSession");
                                    todo!();
                                }
                            }
                        },
                        Ok(n) = self.stream.read(&mut buf) => {
                            match n {
                                0 => {
                                    // client disconnected
                                    self.session_manager_handle.disconnect_client(self.id).await.unwrap();
                                    break;
                                },
                                _ => {
                                    self.session_manager_handle.client_send_user_input(self.id, Bytes::copy_from_slice(&buf[..n])).await.unwrap();
                                }
                            }
                        }
                    }
                }
            }
        });

        Ok(handle_clone)
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

    pub async fn notify_attach_failed(&mut self) -> Result<()> {
        Ok(self.tx.send(ClientEvent::FailedAttachToSession).await?)
    }

    async fn kill(&self) -> crate::error::Result<()> {
        todo!()
    }

    fn is_alive(&self) -> bool {
        todo!()
    }
}

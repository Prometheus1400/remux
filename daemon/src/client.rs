use std::collections::HashMap;

use bytes::Bytes;
use rand::Rng;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    sync::mpsc,
};
use tracing::trace;

use crate::{
    actor::{Actor, ActorHandle},
    session_manager::SessionManagerHandle,
};

pub enum ClientEvent {
    AttachToSession(u32),
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
    pub fn new(stream: UnixStream, session_manager_handle: SessionManagerHandle) -> Self {
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
}
impl Actor<ClientHandle> for Client {
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
                                    self.session_manager_handle.connect_client(self.id, handle.clone(), session_id).await;
                                }
                                SessionOutput(bytes) => {
                                    self.stream.write_all(&bytes).await.unwrap();
                                }
                            }
                        },
                        Ok(n) = self.stream.read(&mut buf) => {
                            self.session_manager_handle.client_send_user_input(self.id, Bytes::copy_from_slice(&buf[..n])).await;
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
impl ClientHandle {
    pub async fn send_session_output(&mut self, bytes: Bytes) {
        self.tx
            .send(ClientEvent::SessionOutput(bytes))
            .await
            .unwrap();
    }

    pub async fn attach_to_session(&mut self, session_id: u32) {
        self.tx.send(ClientEvent::AttachToSession(session_id)).await.unwrap();
    }
}
impl ActorHandle for ClientHandle {
    async fn kill(&self) -> crate::error::Result<()> {
        todo!()
    }

    fn is_alive(&self) -> bool {
        todo!()
    }
}

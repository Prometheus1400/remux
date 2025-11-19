use std::{collections::HashMap, vec};

use bytes::Bytes;
use tokio::sync::mpsc;

use crate::{
    actor::{Actor, ActorHandle},
    client::ClientHandle,
    session::{Session, SessionHandle},
};

use tracing::trace;

pub enum SessionManagerEvent {
    ClientConnect {
        client_id: u32,
        client_handle: ClientHandle,
        session_id: u32,
        create_session: bool
    },
    ClientDisconnect {
        client_id: u32,
    },
    ClientSendUserInput {
        client_id: u32,
        bytes: Bytes,
    },
    SessionSendOutput {
        session_id: u32,
        bytes: Bytes,
    },
}

pub struct SessionManager {
    handle: SessionManagerHandle,
    rx: mpsc::Receiver<SessionManagerEvent>,
    session_table: HashMap<u32, SessionHandle>,
    client_table: HashMap<u32, ClientHandle>,
    session_to_client_mapping: HashMap<u32, Vec<u32>>,
    client_to_session_mapping: HashMap<u32, u32>,
}
impl SessionManager {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(10);
        let handle = SessionManagerHandle { tx };
        let session_one_handle = Session::new(1, handle.clone()).run().unwrap();
        let mut session_table = HashMap::new();
        session_table.insert(1u32, session_one_handle);
        let mut session_to_client_mapping = HashMap::new();
        session_to_client_mapping.insert(1u32, vec![]);
        Self {
            handle,
            rx,
            session_table,
            client_table: HashMap::new(),
            session_to_client_mapping,
            client_to_session_mapping: HashMap::new(),
        }
    }

    async fn handle_client_connect(&mut self, client_id: u32, client_handle: ClientHandle, session_id: u32, create_session: bool) {
        self.client_table.insert(client_id, client_handle);
        let clients = self.session_to_client_mapping.entry(session_id).or_insert(vec![]);
        clients.push(client_id);
        self.client_to_session_mapping.insert(client_id,session_id); 

        let mut session_handle = self.session_table.get_mut(&session_id).unwrap();
        session_handle.send_new_connection().await;
    }
    async fn handle_client_send_user_input(&mut self, client_id: u32, bytes: Bytes) {
        if let Some(session_id) = self.client_to_session_mapping.get(&client_id) {
            let session_handle = self.session_table.get_mut(session_id).unwrap();
            session_handle.send_user_input(bytes).await;
        }
    }
    async fn handle_session_send_output(&mut self, session_id: u32, bytes: Bytes) {
        for client_id in self.session_to_client_mapping.get(&session_id).unwrap() {
            let client_handle = self.client_table.get_mut(client_id).unwrap();
            client_handle.send_session_output(bytes.clone()).await;
        }
    }
}
impl Actor<SessionManagerHandle> for SessionManager {
    fn run(mut self) -> crate::error::Result<SessionManagerHandle> {
        let handle_clone = self.handle.clone();
        let _task = tokio::spawn({
            async move {
                loop {
                    tokio::select! {
                        Some(event) = self.rx.recv() => {
                            use SessionManagerEvent::*;
                            match event {
                                ClientConnect {client_id, client_handle, session_id, create_session} => {
                                    trace!("SessionManager: ClientConnect");
                                    self.handle_client_connect(client_id, client_handle, session_id, create_session).await;
                                },
                                ClientSendUserInput {client_id, bytes} => {
                                    trace!("SessionManager: ClientSendUserInput");
                                    self.handle_client_send_user_input(client_id, bytes).await;
                                },
                                SessionSendOutput {session_id, bytes} => {
                                    trace!("SessionManager: SessionSendOutput");
                                    self.handle_session_send_output(session_id, bytes).await;
                                },
                                ClientDisconnect {..} => {
                                todo!()}
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
pub struct SessionManagerHandle {
    tx: mpsc::Sender<SessionManagerEvent>,
}
// TODO: seperate handle for clients and sessions
impl SessionManagerHandle {
    pub async fn connect_client(
        &self,
        client_id: u32,
        client_handle: ClientHandle,
        session_id: u32,
    ) {
        self.tx
            .send(SessionManagerEvent::ClientConnect {
                client_id,
                client_handle,
                session_id,
                create_session: true
            })
            .await
            .unwrap();
    }
    pub async fn client_send_user_input(&self, client_id: u32, bytes: Bytes) {
        self.tx
            .send(SessionManagerEvent::ClientSendUserInput { client_id, bytes })
            .await
            .unwrap();
    }
    pub async fn session_send_output(&self, session_id: u32, bytes: Bytes) {
        self.tx
            .send(SessionManagerEvent::SessionSendOutput { session_id, bytes })
            .await
            .unwrap();
    }
}
impl ActorHandle for SessionManagerHandle {
    async fn kill(&self) -> crate::error::Result<()> {
        todo!()
    }

    fn is_alive(&self) -> bool {
        todo!()
    }
}

use std::{collections::HashMap, vec};

use bytes::Bytes;
use handle_macro::Handle;
use tokio::sync::mpsc;
use tracing::Instrument;

use crate::{
    actors::{
        client_connection::ClientConnectionHandle,
        session::{Session, SessionHandle},
    },
    error::Result,
    prelude::*,
};

#[allow(unused)]
#[derive(Handle)]
pub enum SessionManagerEvent {
    // client -> session manager events
    ClientConnect {
        client_id: u32,
        client_handle: ClientConnectionHandle,
        session_id: u32,
        create_session: bool,
    },
    ClientDisconnect {
        client_id: u32,
    },
    ClientRequestSwitchSession {
        client_id: u32,
    },
    ClientSwitchSession {
        client_id: u32,
        session_id: u32,
    },

    // client -> session events
    UserInput {
        client_id: u32,
        bytes: Bytes,
    },
    UserSplitPane {
        client_id: u32,
    },
    UserIteratePane {
        client_id: u32,
        is_next: bool,
    },
    UserKillPane {
        client_id: u32,
    },

    // session -> client events
    SessionSendOutput {
        session_id: u32,
        bytes: Bytes,
    },
}
use SessionManagerEvent::*;

pub struct SessionManager {
    handle: SessionManagerHandle,
    rx: mpsc::Receiver<SessionManagerEvent>,
    sessions: HashMap<u32, SessionHandle>,
    clients: HashMap<u32, ClientConnectionHandle>,
    session_to_client_mapping: HashMap<u32, Vec<u32>>, // support multiple clients attached to same session
    client_to_session_mapping: HashMap<u32, u32>,      // one client can only attach to one session
}
impl SessionManager {
    #[instrument]
    pub fn spawn() -> Result<SessionManagerHandle> {
        let session_manager = SessionManager::new();
        session_manager.run()
    }

    #[instrument]
    fn new() -> Self {
        let (tx, rx) = mpsc::channel(10);
        let handle = SessionManagerHandle { tx };
        Self {
            handle,
            rx,
            sessions: HashMap::new(),
            clients: HashMap::new(),
            session_to_client_mapping: HashMap::new(),
            client_to_session_mapping: HashMap::new(),
        }
    }

    #[instrument(skip(self))]
    fn run(mut self) -> crate::error::Result<SessionManagerHandle> {
        let span = tracing::Span::current();
        let handle_clone = self.handle.clone();
        let _task = tokio::spawn({
            async move {
                loop {
                    if let Some(event) = self.rx.recv().await {
                        match event {
                            ClientConnect {
                                client_id,
                                client_handle,
                                session_id,
                                create_session,
                            } => {
                                debug!("SessionManager: ClientConnect");
                                self.handle_client_connect(
                                    client_id,
                                    client_handle,
                                    session_id,
                                    create_session,
                                )
                                .await
                                .unwrap();
                            }
                            ClientDisconnect { client_id } => {
                                debug!("SessionManager: ClientDisconnect");
                                self.handle_client_disconnect(client_id).await.unwrap();
                            }
                            ClientRequestSwitchSession { client_id } => {
                                debug!("SessionManager: ClientRequestSwitchSession");
                                self.handle_client_request_switch_session(client_id)
                                    .await
                                    .unwrap();
                            }
                            ClientSwitchSession {
                                client_id,
                                session_id,
                            } => {
                                debug!("SessionManager: ClientSwitchSession");
                                self.handle_client_switch_session(client_id, session_id)
                                    .await
                                    .unwrap();
                            }
                            UserInput { client_id, bytes } => {
                                trace!("SessionManager: UserInput");
                                self.handle_client_send_user_input(client_id, bytes)
                                    .await
                                    .unwrap();
                            }
                            UserSplitPane { client_id } => {
                                debug!("SessionManager: UserSplitPane");
                                self.handle_client_split_pane(client_id).await.unwrap();
                            }
                            UserIteratePane { client_id, is_next } => {
                                debug!("SessionManager: UserIteratePane");
                                self.handle_client_iterate_pane(client_id, is_next).await.unwrap();
                            }
                            UserKillPane { client_id } => {
                                debug!("SessionManager: UserKillPane");
                                self.handle_client_kill_pane(client_id).await.unwrap();
                            }
                            SessionSendOutput { session_id, bytes } => {
                                trace!("SessionManager: SessionSendOutput");
                                self.handle_session_send_output(session_id, bytes)
                                    .await
                                    .unwrap();
                            }
                        }
                    }
                }
            }
            .instrument(span)
        });

        Ok(handle_clone)
    }

    async fn handle_client_connect(
        &mut self,
        client_id: u32,
        client_handle: ClientConnectionHandle,
        session_id: u32,
        create_session: bool,
    ) -> Result<()> {
        // session doesn't exist either send client error or create it
        if !self.sessions.contains_key(&session_id) {
            if create_session {
                let new_session = Session::spawn(session_id, self.handle.clone()).unwrap();
                self.sessions.insert(session_id, new_session);
            } else {
                client_handle.failed_attach_to_session(session_id).await?;
            }
        }
        // session exists
        self.clients.insert(client_id, client_handle.clone());
        let clients = self
            .session_to_client_mapping
            .entry(session_id)
            .or_insert(vec![]);
        clients.push(client_id);
        self.client_to_session_mapping.insert(client_id, session_id);

        let session_handle = self
            .sessions
            .get_mut(&session_id)
            .expect("session should exist here");
        client_handle.success_attach_to_session(session_id).await?;
        session_handle.user_connection().await
    }
    async fn handle_client_disconnect(&mut self, client_id: u32) -> Result<()> {
        self.clients.remove(&client_id);
        if let Some(session_id) = self.client_to_session_mapping.remove(&client_id) {
            if let Some(clients) = self.session_to_client_mapping.get_mut(&session_id) {
                clients.retain(|c| c != &client_id);
            }
        }
        Ok(())
    }
    async fn handle_client_request_switch_session(&mut self, client_id: u32) -> Result<()> {
        if let Some(client_handle) = self.clients.get(&client_id) {
            client_handle
                .respond_request_switch_session(self.sessions.keys().copied().collect())
                .await?;
        }
        Ok(())
    }
    async fn handle_client_switch_session(
        &mut self,
        client_id: u32,
        session_id: u32,
    ) -> Result<()> {
        let client_handle = self.unmap_client(client_id).unwrap();
        self.map_client(client_id, client_handle, session_id);
        if let Some(session_handle) = self.sessions.get(&session_id) {
            session_handle.redraw().await?;
        }
        // let session_id = self.client_to_session_mapping.get(&client_id).unwrap();
        // let mut clients = self.session_to_client_mapping.get_mut(&session_id).unwrap();
        // clients.retain(|c| c != &client_id);
        // if let Some(client_handle) = self.clients.get(&client_id) {
        //     client_handle
        //         .respond_request_switch_session(self.sessions.keys().copied().collect())
        //         .await?;
        // }
        Ok(())
    }
    async fn handle_client_send_user_input(&mut self, client_id: u32, bytes: Bytes) -> Result<()> {
        if let Some(session_id) = self.client_to_session_mapping.get(&client_id) {
            let session_handle = self.sessions.get_mut(session_id).unwrap();
            session_handle.user_input(bytes).await
        } else {
            Ok(())
        }
    }
    async fn handle_client_kill_pane(&mut self, client_id: u32) -> Result<()> {
        if let Some(session_id) = self.client_to_session_mapping.get(&client_id) {
            let session_handle = self.sessions.get_mut(session_id).unwrap();
            session_handle.user_kill_pane().await
        } else {
            Ok(())
        }
    }
    async fn handle_client_split_pane(&mut self, client_id: u32) -> Result<()> {
        if let Some(session_id) = self.client_to_session_mapping.get(&client_id) {
            let session_handle = self.sessions.get_mut(session_id).unwrap();
            session_handle.user_split_pane().await
        } else {
            // TODO: should error
            Ok(())
        }
    }
    async fn handle_client_iterate_pane(&mut self, client_id: u32, is_next: bool) -> Result<()> {
        if let Some(session_id) = self.client_to_session_mapping.get(&client_id) {
            let session_handle = self.sessions.get_mut(session_id).unwrap();
            session_handle.user_iterate_pane(is_next).await
        } else {
            // TODO: should error
            Ok(())
        }
    }
    async fn handle_session_send_output(&mut self, session_id: u32, bytes: Bytes) -> Result<()> {
        for client_id in self.session_to_client_mapping.get(&session_id).unwrap() {
            let client_handle = self.clients.get_mut(client_id).unwrap();
            client_handle.session_output(bytes.clone()).await?;
        }
        Ok(())
    }

    fn map_client(
        &mut self,
        client_id: u32,
        client_handle: ClientConnectionHandle,
        session_id: u32,
    ) -> Result<()> {
        self.clients.insert(client_id, client_handle);
        let clients = self
            .session_to_client_mapping
            .entry(session_id)
            .or_insert(vec![]);
        clients.push(client_id);
        self.client_to_session_mapping.insert(client_id, session_id);

        // let session_handle = self
        //     .sessions
        //     .get_mut(&session_id)
        //     .expect("session should exist here");
        Ok(())
    }

    fn unmap_client(&mut self, client_id: u32) -> Option<ClientConnectionHandle> {
        let client_handle = self.clients.remove(&client_id)?;
        if let Some(session_id) = self.client_to_session_mapping.remove(&client_id) {
            if let Some(clients) = self.session_to_client_mapping.get_mut(&session_id) {
                clients.retain(|c| c != &client_id);
            }
        }
        Some(client_handle)
    }
}

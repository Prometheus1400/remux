use std::collections::HashMap;

use bytes::Bytes;
use color_eyre::eyre::{self, OptionExt, eyre};
use handle_macro::Handle;
use itertools::Itertools;
use remux_core::states::DaemonState;
use tokio::sync::mpsc;
use tracing::Instrument;
use uuid::Uuid;

use crate::{
    actors::{
        client_connection::ClientConnectionHandle,
        session::{Session, SessionHandle},
    },
    layout::SplitDirection,
    prelude::*,
};

#[allow(unused)]
#[derive(Handle, Debug)]
pub enum SessionManagerEvent {
    // client -> session manager events
    ClientConnect {
        client_id: Uuid,
        client_handle: ClientConnectionHandle,
        session_name: Option<String>,
        create_session: bool,
    },
    ClientDisconnect {
        client_id: Uuid,
    },
    ClientSwitchSession {
        client_id: Uuid,
        session_name: String,
    },

    // client -> session events
    UserInput {
        client_id: Uuid,
        bytes: Bytes,
    },
    UserSplitPane {
        client_id: Uuid,
        direction: SplitDirection,
    },
    UserIteratePane {
        client_id: Uuid,
        is_next: bool,
    },
    UserKillPane {
        client_id: Uuid,
    },

    // session -> client events
    SessionSendOutput {
        session_id: u32,
        bytes: Bytes,
    },
    TerminalResize {
        rows: u16,
        cols: u16,
    },
}
use SessionManagerEvent::*;

#[derive(Debug)]
struct SessionInfo {
    pub handle: SessionHandle,
    pub name: String,
    pub id: u32,
}

#[derive(Debug)]
struct SessionManagerState {
    session_name_to_id: HashMap<String, u32>,
    sessions: HashMap<u32, SessionInfo>,
    session_to_client_mapping: HashMap<u32, Vec<Uuid>>, // support multiple clients attached to same session
    clients: HashMap<Uuid, ClientConnectionHandle>,
    client_to_session_mapping: HashMap<Uuid, u32>, // one client can only attach to one session
    session_id_count: u32,
    manager_handle: SessionManagerHandle,
}

impl SessionManagerState {
    pub fn new(manager_handle: &SessionManagerHandle) -> Self {
        Self {
            session_name_to_id: Default::default(),
            sessions: Default::default(),
            session_to_client_mapping: Default::default(),
            clients: Default::default(),
            client_to_session_mapping: Default::default(),
            session_id_count: Default::default(),
            manager_handle: manager_handle.clone(),
        }
    }
    fn new_session_id(&mut self) -> u32 {
        let x = self.session_id_count;
        self.session_id_count += 1;
        x
    }
    pub fn snapshot(&self) -> DaemonState {
        let mut daemon_state = DaemonState::default();
        daemon_state.set_sessions(self.sessions.values().map(|s| (s.id, s.name.clone())).collect_vec());
        daemon_state
    }
    // pub fn get_by_id(&self, id: u32) -> Option<&SessionInfo> {
    //     self.sessions.get(&id)
    // }
    pub fn get_session_by_name(&self, name: &str) -> Option<&SessionInfo> {
        self.session_name_to_id.get(name).and_then(|id| self.sessions.get(id))
    }
    pub fn get_session_for_client(&self, client_id: &Uuid) -> Result<&SessionInfo> {
        let session_id = self
            .client_to_session_mapping
            .get(client_id)
            .ok_or_eyre("client has no session")?;
        self.sessions.get(session_id).ok_or_eyre("no session")
    }
    pub fn get_clients_for_session(&self, session_id: &u32) -> Result<Vec<&ClientConnectionHandle>> {
        let client_ids = self
            .session_to_client_mapping
            .get(session_id)
            .ok_or_eyre("error getting clients for session")?;
        Ok(self
            .clients
            .iter()
            .filter(|c| client_ids.contains(c.0))
            .map(|c| c.1)
            .collect_vec())
    }

    pub fn create_new_session(&mut self, name: Option<&str>) -> Result<&SessionInfo> {
        if name.and_then(|n| self.get_session_by_name(n)).is_some() {
            Err(eyre!("duplicate session"))
        } else {
            let id = self.new_session_id();
            let name = name.map(|n| n.to_owned()).unwrap_or(id.to_string());
            let handle = Session::spawn(id, name.clone(), self.manager_handle.clone())?;
            self.session_name_to_id.insert(name.clone(), id);
            self.sessions.insert(id, SessionInfo { handle, name, id });
            self.sessions
                .get(&id)
                .ok_or(eyre!("couldn't get session info from sessions"))
        }
    }

    pub fn attach_client(
        &mut self,
        client_id: Uuid,
        client_handle: ClientConnectionHandle,
        session_name: &str,
        create: bool,
    ) -> Result<()> {
        let mut id_opt = self.get_session_by_name(session_name).map(|info| info.id);
        if id_opt.is_none() && create {
            id_opt = Some(self.create_new_session(Some(session_name))?.id);
        }

        if let Some(id) = id_opt {
            self.session_to_client_mapping.entry(id).or_default().push(client_id);
            self.client_to_session_mapping.insert(client_id, id);
            self.clients.insert(client_id, client_handle);
            Ok(())
        } else {
            Err(eyre!("no session to attach client"))
        }
    }
    pub fn detach_client(&mut self, client_id: Uuid) -> Option<ClientConnectionHandle> {
        if self.clients.contains_key(&client_id) {
            let session_id = self.client_to_session_mapping.remove(&client_id)?;
            self.session_to_client_mapping
                .get_mut(&session_id)?
                .retain(|x| x != &client_id);
            self.clients.remove(&client_id)
        } else {
            None
        }
    }
    // pub fn client_switch_session(&mut self, client_id: Uuid, session_name: &str) -> Result<()> {
    //     let id_opt = self.get_by_name(session_name).map(|info| info.id);
    //     if let Some(id) = id_opt {
    //         if let Some(clients) = self.session_to_client_mapping.get_mut(&id) {
    //             clients.retain(|id| id != &client_id);
    //         }
    //         self.session_to_client_mapping
    //             .get_mut(&id)
    //             .ok_or(eyre!("session should exist"))?
    //             .push(client_id);
    //         self.client_to_session_mapping.insert(client_id, id);
    //         Ok(())
    //     } else {
    //         Err(eyre!("no such session to switch to"))
    //     }
    // }
}

#[derive(Debug)]
pub struct SessionManager {
    handle: SessionManagerHandle,
    rx: mpsc::Receiver<SessionManagerEvent>,
    state: SessionManagerState,
}
impl SessionManager {
    pub fn spawn() -> Result<SessionManagerHandle> {
        let session_manager = SessionManager::new();
        session_manager.run()
    }

    fn new() -> Self {
        let (tx, rx) = mpsc::channel(10);
        let handle = SessionManagerHandle { tx };
        Self {
            handle: handle.clone(),
            rx,
            state: SessionManagerState::new(&handle),
        }
    }

    #[instrument(skip(self))]
    fn run(mut self) -> Result<SessionManagerHandle> {
        let handle_clone = self.handle.clone();
        let _task = tokio::spawn({
            async move {
                loop {
                    if let Some(event) = self.rx.recv().await {
                        match &event {
                            SessionSendOutput { .. } | UserInput { .. } => {
                                trace!(event=?event);
                            }
                            _ => {
                                info!(event=?event);
                            }
                        }
                        match event {
                            ClientConnect {
                                client_id,
                                client_handle,
                                session_name,
                                create_session,
                            } => {
                                self.handle_client_connect(
                                    client_id,
                                    client_handle,
                                    session_name.as_deref(),
                                    create_session,
                                )
                                .await
                                .unwrap();
                            }
                            ClientDisconnect { client_id } => {
                                self.handle_client_disconnect(client_id).await.unwrap();
                            }
                            ClientSwitchSession {
                                client_id,
                                session_name,
                            } => {
                                self.handle_client_switch_session(client_id, &session_name)
                                    .await
                                    .unwrap();
                            }
                            UserInput { client_id, bytes } => {
                                self.handle_client_send_user_input(client_id, bytes).await.unwrap();
                            }
                            UserSplitPane { client_id, direction } => {
                                self.handle_client_split_pane(client_id, direction).await.unwrap();
                            }
                            UserIteratePane { client_id, is_next } => {
                                self.handle_client_iterate_pane(client_id, is_next).await.unwrap();
                            }
                            UserKillPane { client_id } => {
                                self.handle_client_kill_pane(client_id).await.unwrap();
                            }
                            SessionSendOutput { session_id, bytes } => {
                                self.handle_session_send_output(session_id, bytes).await.unwrap();
                            }
                            TerminalResize { rows, cols } => {
                                for SessionInfo { handle, .. } in self.state.sessions.values_mut() {
                                    handle.terminal_resize(rows, cols).await.unwrap();
                                }
                            }
                        }
                    }
                }
            }
            .instrument(error_span!(parent: None, "Session Manager"))
        });

        Ok(handle_clone)
    }

    // /// creates a new session and handles updating the state and notifying clients about the update
    // async fn create_session(&mut self, session_name: Option<&str>) -> Result<&SessionInfo> {
    //     self.state.create_new_session(session_name)
    // }

    async fn handle_client_connect(
        &mut self,
        client_id: Uuid,
        client_handle: ClientConnectionHandle,
        session_name: Option<&str>,
        create_session: bool,
    ) -> Result<()> {
        let session_name = session_name.ok_or(eyre!("no session name"))?;
        match self
            .state
            .attach_client(client_id, client_handle.clone(), session_name, create_session)
        {
            Ok(_) => {
                let session_info = self
                    .state
                    .get_session_by_name(session_name)
                    .expect("session should exist here");
                client_handle.initial_attach_result(Ok(self.state.snapshot())).await?;
                client_handle.success_attach_to_session(session_info.id).await?;
                session_info.handle.redraw().await?;
            }
            Err(e) => {
                client_handle.initial_attach_result(Err(eyre::eyre!(e))).await?;
            }
        }
        Ok(())
    }

    async fn handle_client_disconnect(&mut self, client_id: Uuid) -> Result<()> {
        if let Some(client) = self.state.detach_client(client_id) {
            client.disconnect().await
        } else {
            Ok(())
        }
    }

    async fn handle_client_switch_session(&mut self, client_id: Uuid, session_name: &str) -> Result<()> {
        let client = self.state.detach_client(client_id).ok_or_eyre("no such client")?;
        self.state
            .attach_client(client_id, client.clone(), session_name, false)?;
        let session = self.state.get_session_for_client(&client_id)?;
        session.handle.redraw().await?;
        client.success_attach_to_session(session.id).await
    }

    async fn handle_client_send_user_input(&mut self, client_id: Uuid, bytes: Bytes) -> Result<()> {
        self.state
            .get_session_for_client(&client_id)?
            .handle
            .user_input(bytes)
            .await
    }

    async fn handle_client_kill_pane(&mut self, client_id: Uuid) -> Result<()> {
        self.state
            .get_session_for_client(&client_id)?
            .handle
            .user_kill_pane()
            .await
    }

    async fn handle_client_split_pane(&mut self, client_id: Uuid, direction: SplitDirection) -> Result<()> {
        self.state
            .get_session_for_client(&client_id)?
            .handle
            .user_split_pane(direction)
            .await
    }

    async fn handle_client_iterate_pane(&mut self, client_id: Uuid, is_next: bool) -> Result<()> {
        self.state
            .get_session_for_client(&client_id)?
            .handle
            .user_iterate_pane(is_next)
            .await
    }

    async fn handle_session_send_output(&mut self, session_id: u32, bytes: Bytes) -> Result<()> {
        for client in self.state.get_clients_for_session(&session_id)? {
            client.session_output(bytes.clone()).await?;
        }
        Ok(())
    }
}

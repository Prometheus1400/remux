use std::{collections::HashMap, sync::Arc};

use bytes::Bytes;
use color_eyre::eyre::{self, OptionExt, WrapErr, eyre};
use handle_macro::Handle;
use itertools::Itertools;
use tokio::sync::mpsc;
use tracing::Instrument;
use uuid::Uuid;

use crate::{
    actors::{
        client_connection::ClientConnectionHandle,
        session::{Session, SessionHandle},
        window::FocusDirection,
    },
    layout::SplitDirection,
    lua::config::ConfigRuntime,
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
        rows: u16,
        cols: u16,
    },
    ClientDisconnect {
        client_id: Uuid,
    },
    ClientSwitchSession {
        client_id: Uuid,
        session_name: String,
    },
    ClientOpenSessionSwitcher {
        client_id: Uuid,
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
    UserFocusPane {
        client_id: Uuid,
        direction: FocusDirection,
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
    Kill,
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
    session_switcher_state: HashMap<u32, SessionSwitcherState>,
    session_id_count: u32,
    manager_handle: SessionManagerHandle,
    config_runtime: Arc<ConfigRuntime>,
}

#[derive(Debug, Clone)]
struct SessionSwitcherState {
    sessions: Vec<String>,
    selected: usize,
}

impl SessionManagerState {
    pub fn new(manager_handle: &SessionManagerHandle, config_runtime: Arc<ConfigRuntime>) -> Self {
        Self {
            session_name_to_id: Default::default(),
            sessions: Default::default(),
            session_to_client_mapping: Default::default(),
            clients: Default::default(),
            client_to_session_mapping: Default::default(),
            session_switcher_state: Default::default(),
            session_id_count: Default::default(),
            manager_handle: manager_handle.clone(),
            config_runtime,
        }
    }
    fn new_session_id(&mut self) -> u32 {
        let x = self.session_id_count;
        self.session_id_count += 1;
        x
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
    pub fn get_clients_for_session(&self, session_id: &u32) -> Vec<&ClientConnectionHandle> {
        let Some(client_ids) = self.session_to_client_mapping.get(session_id) else {
            return Vec::new();
        };

        self.clients
            .iter()
            .filter(|(client_id, _)| client_ids.contains(client_id))
            .map(|(_, handle)| handle)
            .collect_vec()
    }

    pub fn create_new_session(&mut self, name: Option<&str>, rows: u16, cols: u16) -> Result<&SessionInfo> {
        if name.and_then(|n| self.get_session_by_name(n)).is_some() {
            Err(eyre!("duplicate session"))
        } else {
            let id = self.new_session_id();
            let name = name.map(|n| n.to_owned()).unwrap_or(id.to_string());
            let handle = Session::spawn(
                id,
                name.clone(),
                self.manager_handle.clone(),
                self.config_runtime.clone(),
                rows,
                cols,
            )?;
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
        rows: u16,
        cols: u16,
    ) -> Result<()> {
        let mut id_opt = self.get_session_by_name(session_name).map(|info| info.id);
        if id_opt.is_none() && create {
            id_opt = Some(self.create_new_session(Some(session_name), rows, cols)?.id);
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
        let session_id = self.client_to_session_mapping.remove(&client_id);
        let client = self.clients.remove(&client_id);

        if let Some(session_id) = session_id {
            let mut remove_session_state = false;
            if let Some(client_ids) = self.session_to_client_mapping.get_mut(&session_id) {
                client_ids.retain(|x| x != &client_id);
                if client_ids.is_empty() {
                    remove_session_state = true;
                }
            }
            if remove_session_state {
                self.session_to_client_mapping.remove(&session_id);
                self.session_switcher_state.remove(&session_id);
            }
        }

        client
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
    pub fn spawn(config_runtime: Arc<ConfigRuntime>) -> Result<SessionManagerHandle> {
        let session_manager = SessionManager::new(config_runtime);
        session_manager.run()
    }

    fn new(config_runtime: Arc<ConfigRuntime>) -> Self {
        let (tx, rx) = mpsc::channel(10);
        let handle = SessionManagerHandle { tx };
        Self {
            handle: handle.clone(),
            rx,
            state: SessionManagerState::new(&handle, config_runtime),
        }
    }

    #[instrument(skip(self))]
    fn run(mut self) -> Result<SessionManagerHandle> {
        let handle_clone = self.handle.clone();
        let _task: DaemonTask = tokio::spawn({
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
                        let result = match event {
                            ClientConnect {
                                client_id,
                                client_handle,
                                session_name,
                                create_session,
                                rows,
                                cols,
                            } => {
                                self.handle_client_connect(
                                    client_id,
                                    client_handle,
                                    session_name.as_deref(),
                                    create_session,
                                    rows,
                                    cols,
                                )
                                .await
                            }
                            ClientDisconnect { client_id } => self.handle_client_disconnect(client_id).await,
                            ClientSwitchSession {
                                client_id,
                                session_name,
                            } => self.handle_client_switch_session(client_id, &session_name).await,
                            ClientOpenSessionSwitcher { client_id } => {
                                self.handle_client_open_session_switcher(client_id).await
                            }
                            UserInput { client_id, bytes } => {
                                self.handle_client_send_user_input(client_id, bytes).await
                            }
                            UserSplitPane { client_id, direction } => {
                                self.handle_client_split_pane(client_id, direction).await
                            }
                            UserFocusPane { client_id, direction } => {
                                self.handle_client_focus_pane(client_id, direction).await
                            }
                            UserKillPane { client_id } => self.handle_client_kill_pane(client_id).await,
                            SessionSendOutput { session_id, bytes } => {
                                self.handle_session_send_output(session_id, bytes).await
                            }
                            TerminalResize { rows, cols } => self.handle_terminal_resize(rows, cols).await,
                            Kill => {
                                self.handle_shutdown().await?;
                                break;
                            }
                        };

                        if let Err(e) = result {
                            error!(error=%e, "Session manager event handling failed");
                        }
                    } else {
                        break;
                    }
                }

                Ok(())
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
        rows: u16,
        cols: u16,
    ) -> Result<()> {
        let session_name = session_name.ok_or(eyre!("no session name"))?;
        match self.state.attach_client(
            client_id,
            client_handle.clone(),
            session_name,
            create_session,
            rows,
            cols,
        ) {
            Ok(_) => {
                let session_info = self
                    .state
                    .get_session_by_name(session_name)
                    .ok_or_else(|| eyre!("session {session_name} should exist after attach"))?;
                session_info.handle.terminal_resize(rows, cols).await?;
                client_handle.initial_attach_result(Ok(())).await?;
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
            .attach_client(client_id, client.clone(), session_name, false, 0, 0)?;
        let session = self.state.get_session_for_client(&client_id)?;
        session.handle.redraw().await?;
        client.success_attach_to_session(session.id).await
    }

    async fn handle_client_open_session_switcher(&mut self, client_id: Uuid) -> Result<()> {
        let session_id = self
            .state
            .client_to_session_mapping
            .get(&client_id)
            .copied()
            .ok_or_eyre("client has no attached session")?;
        let sessions = self
            .state
            .sessions
            .values()
            .map(|session| session.name.clone())
            .sorted()
            .collect_vec();
        let current_name = self
            .state
            .sessions
            .get(&session_id)
            .map(|session| session.name.clone())
            .unwrap_or_default();
        let selected = sessions.iter().position(|name| name == &current_name).unwrap_or(0);
        self.state.session_switcher_state.insert(
            session_id,
            SessionSwitcherState {
                sessions: sessions.clone(),
                selected,
            },
        );
        self.state
            .get_session_for_client(&client_id)?
            .handle
            .show_session_switcher(sessions, selected)
            .await?;
        Ok(())
    }

    async fn handle_client_send_user_input(&mut self, client_id: Uuid, bytes: Bytes) -> Result<()> {
        if let Some(session_id) = self.state.client_to_session_mapping.get(&client_id).copied() {
            if self.state.session_switcher_state.contains_key(&session_id) {
                return self.handle_session_switcher_input(client_id, session_id, bytes).await;
            }
        }

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

    async fn handle_client_focus_pane(&mut self, client_id: Uuid, direction: FocusDirection) -> Result<()> {
        self.state
            .get_session_for_client(&client_id)?
            .handle
            .user_focus_pane(direction)
            .await
    }

    async fn handle_session_send_output(&mut self, session_id: u32, bytes: Bytes) -> Result<()> {
        let clients = self.state.get_clients_for_session(&session_id);
        if clients.is_empty() {
            trace!(session_id, "dropping session output because no clients remain attached");
            return Ok(());
        }

        for client in clients {
            if let Err(e) = client.session_output(bytes.clone()).await {
                warn!(error=%e, session_id, "Failed to send session output to client");
            }
        }
        Ok(())
    }

    async fn handle_terminal_resize(&mut self, rows: u16, cols: u16) -> Result<()> {
        for SessionInfo { handle, id, .. } in self.state.sessions.values_mut() {
            if let Err(e) = handle
                .terminal_resize(rows, cols)
                .await
                .wrap_err_with(|| format!("failed to resize session {id}"))
            {
                warn!(error=%e, session_id=*id, "Terminal resize failed for session");
            }
        }
        Ok(())
    }

    async fn handle_session_switcher_input(&mut self, client_id: Uuid, session_id: u32, bytes: Bytes) -> Result<()> {
        enum OverlayInputResult {
            UpdateSelection(usize),
            Confirm(String),
            Cancel,
            Ignore,
        }

        let Some(state) = self.state.session_switcher_state.get_mut(&session_id) else {
            return Ok(());
        };

        let action = match bytes.as_ref() {
            b"\x1b[A" => {
                state.selected = state.selected.saturating_sub(1);
                OverlayInputResult::UpdateSelection(state.selected)
            }
            b"\x1b[B" => {
                if state.selected + 1 < state.sessions.len() {
                    state.selected += 1;
                }
                OverlayInputResult::UpdateSelection(state.selected)
            }
            b"\r" | b"\n" => {
                let target = state.sessions.get(state.selected).cloned().unwrap_or_default();
                OverlayInputResult::Confirm(target)
            }
            b"\x1b" => OverlayInputResult::Cancel,
            _ => OverlayInputResult::Ignore,
        };

        match action {
            OverlayInputResult::UpdateSelection(selected) => {
                self.state
                    .get_session_for_client(&client_id)?
                    .handle
                    .update_session_switcher_selection(selected)
                    .await?;
            }
            OverlayInputResult::Confirm(target) => {
                self.state.session_switcher_state.remove(&session_id);
                self.state
                    .get_session_for_client(&client_id)?
                    .handle
                    .hide_session_switcher()
                    .await?;
                self.handle_client_switch_session(client_id, &target).await?;
            }
            OverlayInputResult::Cancel => {
                self.state.session_switcher_state.remove(&session_id);
                self.state
                    .get_session_for_client(&client_id)?
                    .handle
                    .hide_session_switcher()
                    .await?;
            }
            OverlayInputResult::Ignore => {}
        }

        Ok(())
    }

    async fn handle_shutdown(&mut self) -> Result<()> {
        for SessionInfo { handle, id, .. } in self.state.sessions.values() {
            if let Err(e) = handle
                .kill()
                .await
                .wrap_err_with(|| format!("failed to kill session {id}"))
            {
                warn!(error=%e, session_id=*id, "Session shutdown failed");
            }
        }
        self.state.sessions.clear();
        self.state.session_name_to_id.clear();
        self.state.session_to_client_mapping.clear();
        self.state.client_to_session_mapping.clear();
        self.state.clients.clear();
        self.state.session_switcher_state.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::sync::Arc;

    use bytes::Bytes;
    use tokio::net::UnixStream;
    use uuid::Uuid;

    use super::SessionManager;
    use crate::{actors::client_connection::ClientConnection, lua::config::ConfigRuntime, prelude::Result};

    #[tokio::test]
    async fn get_clients_for_session_returns_empty_when_session_has_no_mapping() -> Result<()> {
        let config_runtime = Arc::new(ConfigRuntime::load()?);
        let session_manager = SessionManager::new(config_runtime);

        assert!(session_manager.state.get_clients_for_session(&42).is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn session_output_after_last_client_disconnect_is_ignored() -> Result<()> {
        let config_runtime = Arc::new(ConfigRuntime::load()?);
        let session_manager_handle = SessionManager::spawn(config_runtime.clone())?;
        let (client_stream, daemon_stream) = UnixStream::pair()?;
        let client_id = Uuid::new_v4();

        let _client = ClientConnection::spawn(
            1,
            client_id,
            daemon_stream,
            session_manager_handle.clone(),
            config_runtime,
            "disconnect-race",
            20,
            60,
        )?;

        drop(client_stream);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        session_manager_handle.client_disconnect(client_id).await?;
        session_manager_handle
            .session_send_output(0, Bytes::from_static(b"late-output"))
            .await?;

        session_manager_handle.kill().await?;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        Ok(())
    }
}

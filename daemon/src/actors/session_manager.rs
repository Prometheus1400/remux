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
    lua::config::{
        ConfigRuntime, LoadedFuzzySelectorWidget, LoadedWidget, LuaRuntimeState, RuntimeCommand, RuntimePaneState,
        RuntimeSessionState, RuntimeWidgetState, RuntimeWindowState,
    },
    prelude::*,
    render::{
        overlay::{FuzzySelectorOverlay, SelectorItem, SelectorOverlay},
        widget::OverlayWidget,
    },
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
    InvokeNamedAction {
        client_id: Uuid,
        name: String,
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
    UserNewWindow {
        client_id: Uuid,
    },
    UserNextWindow {
        client_id: Uuid,
    },
    UserPrevWindow {
        client_id: Uuid,
    },
    UserKillWindow {
        client_id: Uuid,
    },
    UserSelectWindow {
        client_id: Uuid,
        index: usize,
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
    active_widget: HashMap<u32, ActiveWidgetState>,
    session_id_count: u32,
    manager_handle: SessionManagerHandle,
    config_runtime: Arc<ConfigRuntime>,
}

#[derive(Debug, Clone)]
struct ActiveSelectorState {
    widget_id: String,
    overlay: SelectorOverlay,
}

impl ActiveSelectorState {
    fn selected_item(&self) -> Option<&SelectorItem> {
        self.overlay.items.get(self.overlay.selected)
    }
}

#[derive(Debug, Clone)]
struct ActiveFuzzySelectorState {
    widget_id: String,
    title: String,
    footer: String,
    placeholder: String,
    style: crate::render::overlay::FuzzySelectorStyle,
    all_items: Vec<SelectorItem>,
    filtered_indices: Vec<usize>,
    query: String,
    selected: usize,
}

impl ActiveFuzzySelectorState {
    fn from_loaded(widget_id: String, loaded: LoadedFuzzySelectorWidget) -> Self {
        let filtered_indices = (0..loaded.items.len()).collect();
        Self {
            widget_id,
            title: loaded.title,
            footer: loaded.footer,
            placeholder: loaded.placeholder,
            style: loaded.style,
            all_items: loaded.items,
            filtered_indices,
            query: String::new(),
            selected: loaded.selected,
        }
    }

    fn overlay(&self) -> OverlayWidget {
        OverlayWidget::FuzzySelector(FuzzySelectorOverlay {
            title: self.title.clone(),
            footer: self.footer.clone(),
            placeholder: self.placeholder.clone(),
            query: self.query.clone(),
            items: self
                .filtered_indices
                .iter()
                .filter_map(|&index| self.all_items.get(index).cloned())
                .collect(),
            selected: self.selected.min(self.filtered_indices.len().saturating_sub(1)),
            style: self.style.clone(),
        })
    }

    fn selected_item(&self) -> Option<&SelectorItem> {
        let index = *self.filtered_indices.get(self.selected)?;
        self.all_items.get(index)
    }

    fn recompute_matches(&mut self) {
        self.filtered_indices = fuzzy_match_indices(&self.query, &self.all_items);
        self.selected = self.selected.min(self.filtered_indices.len().saturating_sub(1));
    }

    fn append_query(&mut self, ch: char) {
        self.query.push(ch);
        self.selected = 0;
        self.recompute_matches();
    }

    fn delete_query_char(&mut self) {
        self.query.pop();
        self.selected = 0;
        self.recompute_matches();
    }
}

#[derive(Debug, Clone)]
enum ActiveWidgetState {
    Selector(ActiveSelectorState),
    FuzzySelector(ActiveFuzzySelectorState),
}

impl ActiveWidgetState {
    fn overlay(&self) -> OverlayWidget {
        match self {
            Self::Selector(state) => OverlayWidget::Selector(state.overlay.clone()),
            Self::FuzzySelector(state) => state.overlay(),
        }
    }

    fn widget_id(&self) -> &str {
        match self {
            Self::Selector(state) => &state.widget_id,
            Self::FuzzySelector(state) => &state.widget_id,
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::Selector(_) => "selector",
            Self::FuzzySelector(_) => "fuzzy-selector",
        }
    }

    fn selected_item_id(&self) -> Option<String> {
        match self {
            Self::Selector(state) => state.selected_item().map(|item| item.id.clone()),
            Self::FuzzySelector(state) => state.selected_item().map(|item| item.id.clone()),
        }
    }
}

#[derive(Debug, Clone)]
struct SessionContext {
    current_session: Option<RuntimeSessionState>,
    current_window: Option<RuntimeWindowState>,
    current_pane: Option<RuntimePaneState>,
    sessions: Vec<RuntimeSessionState>,
    active_widget: Option<RuntimeWidgetState>,
}

impl SessionContext {
    fn into_lua_state(self) -> LuaRuntimeState {
        LuaRuntimeState {
            sessions: self.sessions,
            current_session: self.current_session,
            current_window: self.current_window,
            current_pane: self.current_pane,
            active_widget: self.active_widget,
        }
    }
}

#[derive(Debug, Clone)]
enum OverlayInputResult {
    Updated,
    Confirm,
    Cancel,
    Ignore,
}

impl SessionManagerState {
    pub fn new(manager_handle: &SessionManagerHandle, config_runtime: Arc<ConfigRuntime>) -> Self {
        Self {
            session_name_to_id: Default::default(),
            sessions: Default::default(),
            session_to_client_mapping: Default::default(),
            clients: Default::default(),
            client_to_session_mapping: Default::default(),
            active_widget: Default::default(),
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
                self.active_widget.remove(&session_id);
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
                            InvokeNamedAction { client_id, name } => {
                                self.handle_invoke_named_action(client_id, &name).await
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
                            UserNewWindow { client_id } => self.handle_client_new_window(client_id).await,
                            UserNextWindow { client_id } => self.handle_client_next_window(client_id).await,
                            UserPrevWindow { client_id } => self.handle_client_prev_window(client_id).await,
                            UserKillWindow { client_id } => self.handle_client_kill_window(client_id).await,
                            UserSelectWindow { client_id, index } => {
                                self.handle_client_select_window(client_id, index).await
                            }
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
        self.handle_client_open_widget(client_id, "session_switcher").await
    }

    async fn handle_client_send_user_input(&mut self, client_id: Uuid, bytes: Bytes) -> Result<()> {
        if let Some(session_id) = self.state.client_to_session_mapping.get(&client_id).copied() {
            if self.state.active_widget.contains_key(&session_id) {
                return self.handle_active_widget_input(client_id, session_id, bytes).await;
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

    async fn handle_client_new_window(&mut self, client_id: Uuid) -> Result<()> {
        self.state
            .get_session_for_client(&client_id)?
            .handle
            .user_new_window()
            .await
    }

    async fn handle_client_next_window(&mut self, client_id: Uuid) -> Result<()> {
        self.state
            .get_session_for_client(&client_id)?
            .handle
            .user_next_window()
            .await
    }

    async fn handle_client_prev_window(&mut self, client_id: Uuid) -> Result<()> {
        self.state
            .get_session_for_client(&client_id)?
            .handle
            .user_prev_window()
            .await
    }

    async fn handle_client_kill_window(&mut self, client_id: Uuid) -> Result<()> {
        self.state
            .get_session_for_client(&client_id)?
            .handle
            .user_kill_window()
            .await
    }

    async fn handle_client_select_window(&mut self, client_id: Uuid, index: usize) -> Result<()> {
        self.state
            .get_session_for_client(&client_id)?
            .handle
            .user_select_window(index)
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

    async fn handle_invoke_named_action(&mut self, client_id: Uuid, name: &str) -> Result<()> {
        let context = self.build_session_context(client_id).await?;
        let commands = self
            .state
            .config_runtime
            .invoke_named_action(name, &context.clone().into_lua_state())?;
        self.execute_runtime_commands(client_id, commands).await
    }

    async fn handle_client_open_widget(&mut self, client_id: Uuid, widget_id: &str) -> Result<()> {
        let session_id = self
            .state
            .client_to_session_mapping
            .get(&client_id)
            .copied()
            .ok_or_eyre("client has no attached session")?;
        let context = self.build_session_context(client_id).await?;
        let widget = self
            .state
            .config_runtime
            .load_widget(widget_id, &context.into_lua_state())?;

        let active_widget = match widget {
            LoadedWidget::Selector(overlay) => ActiveWidgetState::Selector(ActiveSelectorState {
                widget_id: widget_id.to_owned(),
                overlay,
            }),
            LoadedWidget::FuzzySelector(loaded) => {
                ActiveWidgetState::FuzzySelector(ActiveFuzzySelectorState::from_loaded(widget_id.to_owned(), loaded))
            }
        };
        let overlay = active_widget.overlay();
        self.state.active_widget.insert(session_id, active_widget);
        self.state
            .get_session_for_client(&client_id)?
            .handle
            .show_widget(overlay)
            .await?;
        Ok(())
    }

    async fn handle_active_widget_input(&mut self, client_id: Uuid, session_id: u32, bytes: Bytes) -> Result<()> {
        let Some(state) = self.state.active_widget.get_mut(&session_id) else {
            return Ok(());
        };

        let action = match state {
            ActiveWidgetState::Selector(state) => match bytes.as_ref() {
                b"\x1b[A" | b"k" | b"\x10" => {
                    state.overlay.selected = state.overlay.selected.saturating_sub(1);
                    OverlayInputResult::Updated
                }
                b"\x1b[B" | b"j" | b"\x0e" => {
                    if state.overlay.selected + 1 < state.overlay.items.len() {
                        state.overlay.selected += 1;
                    }
                    OverlayInputResult::Updated
                }
                b"\r" | b"\n" => OverlayInputResult::Confirm,
                b"\x1b" => OverlayInputResult::Cancel,
                _ => OverlayInputResult::Ignore,
            },
            ActiveWidgetState::FuzzySelector(state) => match bytes.as_ref() {
                b"\x1b[A" | b"\x10" => {
                    state.selected = state.selected.saturating_sub(1);
                    OverlayInputResult::Updated
                }
                b"\x1b[B" | b"\x0e" => {
                    if state.selected + 1 < state.filtered_indices.len() {
                        state.selected += 1;
                    }
                    OverlayInputResult::Updated
                }
                b"\r" | b"\n" => OverlayInputResult::Confirm,
                b"\x1b" => OverlayInputResult::Cancel,
                b"\x08" | b"\x7f" => {
                    state.delete_query_char();
                    OverlayInputResult::Updated
                }
                [byte] if (0x20..=0x7e).contains(byte) => {
                    state.append_query(char::from(*byte));
                    OverlayInputResult::Updated
                }
                _ => OverlayInputResult::Ignore,
            },
        };

        match action {
            OverlayInputResult::Updated => {
                let overlay = self
                    .state
                    .active_widget
                    .get(&session_id)
                    .ok_or_else(|| eyre!("active widget disappeared"))?
                    .overlay();
                self.state
                    .get_session_for_client(&client_id)?
                    .handle
                    .update_widget(overlay)
                    .await?;
            }
            OverlayInputResult::Confirm => {
                let (widget_id, selected_id) = {
                    let state = self
                        .state
                        .active_widget
                        .get(&session_id)
                        .ok_or_else(|| eyre!("active widget missing"))?;
                    (state.widget_id().to_owned(), state.selected_item_id())
                };

                let Some(selected_id) = selected_id else {
                    return Ok(());
                };

                self.state.active_widget.remove(&session_id);
                self.state
                    .get_session_for_client(&client_id)?
                    .handle
                    .hide_widget()
                    .await?;
                let context = self.build_session_context(client_id).await?;
                let commands = self.state.config_runtime.invoke_widget_confirm(
                    &widget_id,
                    &selected_id,
                    &context.into_lua_state(),
                )?;
                self.execute_runtime_commands(client_id, commands).await?;
            }
            OverlayInputResult::Cancel => {
                self.state.active_widget.remove(&session_id);
                self.state
                    .get_session_for_client(&client_id)?
                    .handle
                    .hide_widget()
                    .await?;
            }
            OverlayInputResult::Ignore => {}
        }

        Ok(())
    }

    async fn build_session_context(&self, client_id: Uuid) -> Result<SessionContext> {
        let current_session_id = self.state.client_to_session_mapping.get(&client_id).copied();
        let mut sessions = Vec::new();
        for session in self.state.sessions.values() {
            let mut snapshot = session.handle.snapshot().await?;
            snapshot.is_current = Some(snapshot.id) == current_session_id;
            sessions.push(snapshot);
        }
        sessions.sort_by(|a, b| a.name.cmp(&b.name));
        let current_session = sessions.iter().find(|session| session.is_current).cloned();
        let current_window = current_session
            .as_ref()
            .and_then(|session| session.current_window.clone());
        let current_pane = current_window.as_ref().and_then(|window| window.active_pane.clone());
        let active_widget = current_session_id.and_then(|session_id| {
            self.state
                .active_widget
                .get(&session_id)
                .map(|widget| RuntimeWidgetState {
                    id: widget.widget_id().to_owned(),
                    kind: widget.kind().to_owned(),
                })
        });
        Ok(SessionContext {
            current_session,
            current_window,
            current_pane,
            sessions,
            active_widget,
        })
    }

    async fn execute_runtime_commands(&mut self, client_id: Uuid, commands: Vec<RuntimeCommand>) -> Result<()> {
        for command in commands {
            match command {
                RuntimeCommand::Builtin(action) => self.execute_builtin_action(client_id, action).await?,
                RuntimeCommand::OpenWidget(widget_id) => self.handle_client_open_widget(client_id, &widget_id).await?,
                RuntimeCommand::SwitchSession(session_name) => {
                    self.handle_client_switch_session(client_id, &session_name).await?
                }
            }
        }
        Ok(())
    }

    async fn execute_builtin_action(
        &mut self,
        client_id: Uuid,
        action: remux_core::config::BuiltinAction,
    ) -> Result<()> {
        use remux_core::config::BuiltinAction;

        match action {
            BuiltinAction::SplitPaneVertical => {
                self.handle_client_split_pane(client_id, SplitDirection::Vertical).await
            }
            BuiltinAction::SplitPaneHorizontal => {
                self.handle_client_split_pane(client_id, SplitDirection::Horizontal)
                    .await
            }
            BuiltinAction::FocusPaneLeft => self.handle_client_focus_pane(client_id, FocusDirection::Left).await,
            BuiltinAction::FocusPaneDown => self.handle_client_focus_pane(client_id, FocusDirection::Down).await,
            BuiltinAction::FocusPaneUp => self.handle_client_focus_pane(client_id, FocusDirection::Up).await,
            BuiltinAction::FocusPaneRight => self.handle_client_focus_pane(client_id, FocusDirection::Right).await,
            BuiltinAction::KillPane => self.handle_client_kill_pane(client_id).await,
            BuiltinAction::NewWindow => self.handle_client_new_window(client_id).await,
            BuiltinAction::NextWindow => self.handle_client_next_window(client_id).await,
            BuiltinAction::PrevWindow => self.handle_client_prev_window(client_id).await,
            BuiltinAction::KillWindow => self.handle_client_kill_window(client_id).await,
            BuiltinAction::SelectWindow1 => self.handle_client_select_window(client_id, 1).await,
            BuiltinAction::SelectWindow2 => self.handle_client_select_window(client_id, 2).await,
            BuiltinAction::SelectWindow3 => self.handle_client_select_window(client_id, 3).await,
            BuiltinAction::SelectWindow4 => self.handle_client_select_window(client_id, 4).await,
            BuiltinAction::SelectWindow5 => self.handle_client_select_window(client_id, 5).await,
            BuiltinAction::SelectWindow6 => self.handle_client_select_window(client_id, 6).await,
            BuiltinAction::SelectWindow7 => self.handle_client_select_window(client_id, 7).await,
            BuiltinAction::SelectWindow8 => self.handle_client_select_window(client_id, 8).await,
            BuiltinAction::SelectWindow9 => self.handle_client_select_window(client_id, 9).await,
            BuiltinAction::Detach => self.handle_client_disconnect(client_id).await,
            BuiltinAction::OpenSessionSwitcher => self.handle_client_open_widget(client_id, "session_switcher").await,
        }
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
        self.state.active_widget.clear();
        Ok(())
    }
}

fn fuzzy_match_indices(query: &str, items: &[SelectorItem]) -> Vec<usize> {
    if query.is_empty() {
        return (0..items.len()).collect();
    }

    let query_chars = query.to_ascii_lowercase().chars().collect::<Vec<_>>();
    let mut matches = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| fuzzy_score(&query_chars, &item.label).map(|score| (index, score)))
        .collect_vec();

    matches.sort_by_key(|(index, score)| (*score, *index));
    matches.into_iter().map(|(index, _)| index).collect()
}

fn fuzzy_score(query: &[char], label: &str) -> Option<(u8, usize, usize, usize)> {
    let label_chars = label.to_ascii_lowercase().chars().collect::<Vec<_>>();
    let mut positions = Vec::with_capacity(query.len());
    let mut cursor = 0usize;

    for needle in query {
        let found = label_chars[cursor..]
            .iter()
            .position(|candidate| candidate == needle)
            .map(|offset| cursor + offset)?;
        positions.push(found);
        cursor = found + 1;
    }

    let first = *positions.first()?;
    let last = *positions.last()?;
    let contiguous = positions.windows(2).all(|pair| pair[1] == pair[0] + 1);
    let span = last.saturating_sub(first);

    Some((u8::from(!contiguous), span, first, label_chars.len()))
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

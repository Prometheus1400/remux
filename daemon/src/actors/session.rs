use std::collections::BTreeMap;

use bytes::Bytes;
use color_eyre::eyre::WrapErr;
use handle_macro::Handle;
use tokio::sync::mpsc;
use tracing::{Instrument, Span};

use crate::{
    actors::{
        pane::{Pane, PaneHandle},
        session_manager::SessionManagerHandle,
        window::{Window, WindowAction},
    },
    layout::{Rect, SplitDirection},
    prelude::*,
};

#[allow(unused)]
#[derive(Handle, Debug)]
pub enum SessionEvent {
    UserInput(Bytes),
    UserConnection,
    UserSplitPane {
        direction: SplitDirection,
    },
    UserIteratePane {
        is_next: bool,
    },
    UserKillPane,
    Redraw,
    RenameSession(String),
    PaneOutput {
        id: usize,
        bytes: Bytes,
        cursor: Option<(u16, u16)>,
    },
    PaneDied {
        id: usize,
    },
    TerminalResize {
        rows: u16,
        cols: u16,
    },
    Kill,
}
use SessionEvent::*;

pub struct Session {
    id: u32,
    name: String,
    handle: SessionHandle,
    session_manager_handle: SessionManagerHandle,
    rx: mpsc::Receiver<SessionEvent>,
    window: Window,
    pane_handles: BTreeMap<usize, PaneHandle>,
    startup_actions: Vec<WindowAction>,
}

impl Session {
    #[instrument(parent=None, skip(session_manager_handle), name="Session")]
    pub fn spawn(id: u32, name: String, session_manager_handle: SessionManagerHandle) -> Result<SessionHandle> {
        let session = Session::new(id, name, session_manager_handle)?;
        session.run()
    }

    fn new(id: u32, name: String, session_manager_handle: SessionManagerHandle) -> Result<Self> {
        let (tx, rx) = mpsc::channel(10);
        let handle = SessionHandle { tx };
        let (window, startup_actions) = Window::new()?;

        Ok(Self {
            id,
            name,
            session_manager_handle,
            handle,
            rx,
            window,
            pane_handles: BTreeMap::new(),
            startup_actions,
        })
    }

    fn run(mut self) -> Result<SessionHandle> {
        let handle_clone = self.handle.clone();
        let _task: DaemonTask = tokio::spawn(
            async move {
                let startup_actions = std::mem::take(&mut self.startup_actions);
                self.execute_window_actions(startup_actions)
                    .await
                    .wrap_err("failed to execute session startup actions")?;

                loop {
                    if let Some(event) = self.rx.recv().await {
                        match &event {
                            PaneOutput { .. } | UserInput(..) => {
                                trace!(event=?event);
                            }
                            _ => {
                                info!(event=?event);
                            }
                        }

                        match event {
                            Kill => {
                                self.kill_all_panes().await;
                                break;
                            }
                            RenameSession(name) => {
                                let span = Span::current();
                                self.name = name.clone();
                                span.record("name", name);
                            }
                            other => {
                                let result = match other {
                                    UserInput(bytes) => self.handle_user_input(bytes).await,
                                    UserConnection => self.handle_new_connection().await,
                                    UserSplitPane { direction } => self.handle_split_pane(direction).await,
                                    UserIteratePane { is_next } => self.handle_iterate_pane(is_next).await,
                                    UserKillPane => self.handle_kill_pane().await,
                                    Redraw => {
                                        let actions = self.window.redraw()?;
                                        self.execute_window_actions(actions).await
                                    }
                                    TerminalResize { rows, cols } => self.handle_terminal_resize(rows, cols).await,
                                    PaneOutput { id, bytes, cursor } => {
                                        self.handle_pane_output(id, bytes, cursor).await
                                    }
                                    PaneDied { id } => self.handle_pane_died(id).await,
                                    RenameSession(..) | Kill => Ok(()),
                                };

                                if let Err(e) = result {
                                    error!(error=%e, session_id=self.id, "Session event handling failed");
                                }
                            }
                        }
                    } else {
                        break;
                    }
                }

                Ok(())
            }
            .in_current_span(),
        );

        Ok(handle_clone)
    }

    async fn handle_user_input(&mut self, bytes: Bytes) -> Result<()> {
        let actions = self.window.route_input_to_active_pane(bytes)?;
        self.execute_window_actions(actions).await
    }

    async fn handle_new_connection(&mut self) -> Result<()> {
        let actions = self.window.redraw()?;
        self.execute_window_actions(actions).await
    }

    async fn handle_iterate_pane(&mut self, is_next: bool) -> Result<()> {
        let actions = self.window.iterate_active_pane(is_next)?;
        self.execute_window_actions(actions).await
    }

    async fn handle_split_pane(&mut self, direction: SplitDirection) -> Result<()> {
        let actions = self.window.split_active_pane(direction)?;
        self.execute_window_actions(actions).await
    }

    async fn handle_kill_pane(&mut self) -> Result<()> {
        let actions = self.window.kill_active_pane()?;
        self.execute_window_actions(actions).await
    }

    async fn handle_terminal_resize(&mut self, rows: u16, cols: u16) -> Result<()> {
        let actions = self.window.resize_terminal(rows, cols)?;
        self.execute_window_actions(actions).await
    }

    async fn handle_pane_output(&mut self, id: usize, bytes: Bytes, cursor: Option<(u16, u16)>) -> Result<()> {
        let actions = self.window.handle_pane_output(id, bytes, cursor)?;
        self.execute_window_actions(actions).await
    }

    async fn handle_pane_died(&mut self, id: usize) -> Result<()> {
        self.pane_handles.remove(&id);
        let actions = self.window.remove_pane(id)?;
        self.execute_window_actions(actions).await
    }

    async fn execute_window_actions(&mut self, actions: Vec<WindowAction>) -> Result<()> {
        for action in actions {
            match action {
                WindowAction::SendOutput(bytes) => {
                    self.session_manager_handle.session_send_output(self.id, bytes).await?;
                }
                WindowAction::SendInputToPane { id, bytes } => {
                    if let Some(pane) = self.pane_handles.get(&id) {
                        pane.user_input(bytes).await?;
                    }
                }
                WindowAction::ResizePane { id, rect } => {
                    if let Some(pane) = self.pane_handles.get(&id) {
                        pane.resize(rect).await?;
                    }
                }
                WindowAction::KillPane { id } => {
                    if let Some(pane) = self.pane_handles.get(&id) {
                        pane.kill().await?;
                    }
                }
                WindowAction::RerenderPane { id } => {
                    if let Some(pane) = self.pane_handles.get(&id) {
                        pane.rerender().await?;
                    }
                }
                WindowAction::SpawnPane { id, rect } => {
                    let pane = self.spawn_pane(id, rect)?;
                    self.pane_handles.insert(id, pane);
                }
            }
        }

        Ok(())
    }

    fn spawn_pane(&self, id: usize, rect: Rect) -> Result<PaneHandle> {
        Pane::spawn(self.handle.clone(), id, rect)
    }

    async fn kill_all_panes(&self) {
        for (id, pane) in &self.pane_handles {
            if let Err(e) = pane.kill().await {
                warn!(error=%e, pane_id=*id, session_id=self.id, "Failed to kill pane during session shutdown");
            }
        }
    }
}

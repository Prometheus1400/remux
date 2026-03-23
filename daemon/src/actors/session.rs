use std::{collections::BTreeMap, sync::Arc};

use bytes::Bytes;
use color_eyre::eyre::WrapErr;
use handle_macro::Handle;
use tokio::{
    sync::{mpsc, oneshot},
    time::MissedTickBehavior,
};
use tracing::{Instrument, Span};

use crate::{
    actors::{
        pane::{Pane, PaneHandle},
        session_manager::SessionManagerHandle,
        window::{FocusDirection, Window, WindowAction},
    },
    layout::{Rect, SplitDirection},
    lua::config::{ConfigRuntime, RuntimePaneState, RuntimeRectState, RuntimeSessionState, RuntimeWindowState},
    prelude::*,
    render::{
        bar::{BarRenderState, WindowTab},
        diff::render_surface_diff,
        surface::Surface,
        widget::{
            DockedWidgetLayout, OverlayWidget, compute_docked_layout, render_docked_widget, render_overlay_widget,
        },
    },
};

#[allow(unused)]
#[derive(Handle, Debug)]
pub enum SessionEvent {
    UserInput(Bytes),
    UserConnection,
    UserSplitPane {
        direction: SplitDirection,
    },
    UserFocusPane {
        direction: FocusDirection,
    },
    UserKillPane,
    UserNewWindow,
    UserNextWindow,
    UserPrevWindow,
    UserKillWindow,
    UserSelectWindow {
        index: usize,
    },
    Redraw,
    RenameSession(String),
    PaneOutput {
        id: usize,
        surface: Surface,
        cursor: Option<(u16, u16)>,
    },
    PaneDied {
        id: usize,
    },
    TerminalResize {
        rows: u16,
        cols: u16,
    },
    ShowWidget {
        overlay: OverlayWidget,
    },
    UpdateWidget {
        overlay: OverlayWidget,
    },
    HideWidget,
    GetSnapshot {
        reply_to: oneshot::Sender<RuntimeSessionState>,
    },
    Kill,
}
use SessionEvent::*;

const DOCKED_WIDGET_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

#[derive(Debug)]
struct SessionWindow {
    name: String,
    window: Window,
    pane_handles: BTreeMap<usize, PaneHandle>,
    pane_surfaces: BTreeMap<usize, Surface>,
}

impl SessionWindow {
    fn new(name: String, window: Window) -> Self {
        Self {
            name,
            window,
            pane_handles: BTreeMap::new(),
            pane_surfaces: BTreeMap::new(),
        }
    }
}

impl SessionHandle {
    pub async fn snapshot(&self) -> Result<RuntimeSessionState> {
        let (reply_to, rx) = oneshot::channel();
        self.get_snapshot(reply_to).await?;
        rx.await.wrap_err("session snapshot response dropped")
    }
}

pub struct Session {
    id: u32,
    name: String,
    handle: SessionHandle,
    session_manager_handle: SessionManagerHandle,
    rx: mpsc::Receiver<SessionEvent>,
    windows: BTreeMap<u32, SessionWindow>,
    window_order: Vec<u32>,
    active_window_id: u32,
    next_window_id: u32,
    next_pane_id: usize,
    prev_surface: Surface,
    terminal_rows: u16,
    terminal_cols: u16,
    config_runtime: Arc<ConfigRuntime>,
    widget_overlay: Option<OverlayWidget>,
    startup_actions: Vec<WindowAction>,
}

impl Session {
    #[instrument(parent=None, skip(session_manager_handle, config_runtime), name="Session")]
    pub fn spawn(
        id: u32,
        name: String,
        session_manager_handle: SessionManagerHandle,
        config_runtime: Arc<ConfigRuntime>,
        rows: u16,
        cols: u16,
    ) -> Result<SessionHandle> {
        let session = Session::new(id, name, session_manager_handle, config_runtime, rows, cols)?;
        session.run()
    }

    fn new(
        id: u32,
        name: String,
        session_manager_handle: SessionManagerHandle,
        config_runtime: Arc<ConfigRuntime>,
        rows: u16,
        cols: u16,
    ) -> Result<Self> {
        let (tx, rx) = mpsc::channel(10);
        let handle = SessionHandle { tx };
        let initial_window_id = 0;
        let initial_pane_id = 0;
        let (window, startup_actions) = Window::new(
            rows,
            cols,
            Self::content_rect_for(&config_runtime, rows, cols),
            initial_pane_id,
        )?;

        let mut windows = BTreeMap::new();
        windows.insert(initial_window_id, SessionWindow::new("shell".to_owned(), window));

        Ok(Self {
            id,
            name,
            session_manager_handle,
            handle,
            rx,
            windows,
            window_order: vec![initial_window_id],
            active_window_id: initial_window_id,
            next_window_id: 1,
            next_pane_id: 1,
            prev_surface: Surface::new(cols, rows),
            terminal_rows: rows,
            terminal_cols: cols,
            config_runtime,
            widget_overlay: None,
            startup_actions,
        })
    }

    fn run(mut self) -> Result<SessionHandle> {
        let handle_clone = self.handle.clone();
        let _task: DaemonTask = tokio::spawn(
            async move {
                let startup_actions = std::mem::take(&mut self.startup_actions);
                self.execute_window_actions_for_active(startup_actions)
                    .await
                    .wrap_err("failed to execute session startup actions")?;
                self.compose_and_send(true).await?;

                let has_docked_widgets = !self.config_runtime.docked_widgets().is_empty();
                let mut docked_widget_tick = tokio::time::interval(DOCKED_WIDGET_REFRESH_INTERVAL);
                docked_widget_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
                docked_widget_tick.tick().await;

                loop {
                    tokio::select! {
                        _ = docked_widget_tick.tick(), if has_docked_widgets => {
                            if let Err(e) = self.compose_and_send(false).await {
                                error!(error=%e, session_id=self.id, "Docked widget refresh failed");
                            }
                        }
                        event = self.rx.recv() => {
                            if let Some(event) = event {
                                match &event {
                                    PaneOutput { .. } | UserInput(..) => trace!(event=?event),
                                    _ => info!(event=?event),
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
                                            UserFocusPane { direction } => self.handle_focus_pane(direction).await,
                                            UserKillPane => self.handle_kill_pane().await,
                                            UserNewWindow => self.handle_new_window().await,
                                            UserNextWindow => self.handle_cycle_window(1).await,
                                            UserPrevWindow => self.handle_cycle_window(-1).await,
                                            UserKillWindow => self.handle_kill_window().await,
                                            UserSelectWindow { index } => self.handle_select_window(index).await,
                                            Redraw => self.handle_redraw().await,
                                            TerminalResize { rows, cols } => self.handle_terminal_resize(rows, cols).await,
                                            ShowWidget { overlay } => self.handle_show_widget(overlay).await,
                                            UpdateWidget { overlay } => self.handle_update_widget(overlay).await,
                                            HideWidget => self.handle_hide_widget().await,
                                            GetSnapshot { reply_to } => self.handle_get_snapshot(reply_to).await,
                                            PaneOutput { id, surface, cursor } => {
                                                self.handle_pane_output(id, surface, cursor).await
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
                    }
                }

                Ok(())
            }
            .in_current_span(),
        );

        Ok(handle_clone)
    }

    async fn handle_user_input(&mut self, bytes: Bytes) -> Result<()> {
        let actions = self.active_window_mut()?.window.route_input_to_active_pane(bytes)?;
        self.execute_window_actions_for_active(actions).await
    }

    async fn handle_new_connection(&mut self) -> Result<()> {
        self.redraw_active_window().await?;
        self.compose_and_send(true).await
    }

    async fn handle_focus_pane(&mut self, direction: FocusDirection) -> Result<()> {
        let actions = self.active_window_mut()?.window.focus_pane(direction)?;
        self.execute_window_actions_for_active(actions).await?;
        self.compose_and_send(true).await
    }

    async fn handle_split_pane(&mut self, direction: SplitDirection) -> Result<()> {
        let new_pane_id = self.next_pane_id;
        self.next_pane_id += 1;
        let actions = self
            .active_window_mut()?
            .window
            .split_active_pane(new_pane_id, direction)?;
        self.execute_window_actions_for_active(actions).await?;
        self.compose_and_send(true).await
    }

    async fn handle_kill_pane(&mut self) -> Result<()> {
        let actions = self.active_window_mut()?.window.kill_active_pane()?;
        self.execute_window_actions_for_active(actions).await
    }

    async fn handle_new_window(&mut self) -> Result<()> {
        self.hide_window_panes(self.active_window_id).await?;

        let window_id = self.next_window_id;
        self.next_window_id += 1;
        let pane_id = self.next_pane_id;
        self.next_pane_id += 1;
        let (window, startup_actions) =
            Window::new(self.terminal_rows, self.terminal_cols, self.content_rect(), pane_id)?;
        self.windows
            .insert(window_id, SessionWindow::new("shell".to_owned(), window));
        self.window_order.push(window_id);
        self.active_window_id = window_id;
        self.execute_window_actions_for_window(window_id, startup_actions)
            .await?;
        self.reveal_window_panes(window_id).await?;
        self.redraw_active_window().await?;
        self.compose_and_send(true).await
    }

    async fn handle_cycle_window(&mut self, delta: isize) -> Result<()> {
        if self.window_order.len() <= 1 {
            return Ok(());
        }
        let current_index = self
            .window_order
            .iter()
            .position(|window_id| *window_id == self.active_window_id)
            .ok_or_else(|| Error::msg("active window missing from order"))?;
        let len = self.window_order.len() as isize;
        let next_index = ((current_index as isize + delta).rem_euclid(len)) as usize;
        let next_window_id = self.window_order[next_index];
        self.activate_window(next_window_id).await
    }

    async fn handle_select_window(&mut self, index: usize) -> Result<()> {
        if index == 0 {
            return Ok(());
        }
        let Some(&window_id) = self.window_order.get(index - 1) else {
            return Ok(());
        };
        self.activate_window(window_id).await
    }

    async fn handle_kill_window(&mut self) -> Result<()> {
        if self.window_order.len() <= 1 {
            warn!(session_id = self.id, "Can't kill last window");
            return Ok(());
        }

        let active_window_id = self.active_window_id;
        let active_index = self
            .window_order
            .iter()
            .position(|window_id| *window_id == active_window_id)
            .ok_or_else(|| Error::msg("active window missing from order"))?;
        let next_index = if active_index + 1 < self.window_order.len() {
            active_index + 1
        } else {
            active_index.saturating_sub(1)
        };
        let next_window_id = self.window_order[next_index];

        let removed = self
            .windows
            .remove(&active_window_id)
            .ok_or_else(|| Error::msg("active window missing"))?;
        self.window_order.retain(|window_id| *window_id != active_window_id);

        for pane in removed.pane_handles.into_values() {
            pane.kill().await?;
        }

        self.active_window_id = next_window_id;
        self.reveal_window_panes(next_window_id).await?;
        self.redraw_active_window().await?;
        self.compose_and_send(true).await
    }

    async fn handle_redraw(&mut self) -> Result<()> {
        self.redraw_active_window().await?;
        self.compose_and_send(true).await
    }

    async fn handle_terminal_resize(&mut self, rows: u16, cols: u16) -> Result<()> {
        self.terminal_rows = rows;
        self.terminal_cols = cols;
        self.prev_surface = Surface::new(cols, rows);
        let content_rect = self.content_rect();

        let window_ids = self.window_order.clone();
        for window_id in window_ids {
            let actions = {
                let window = self
                    .windows
                    .get_mut(&window_id)
                    .ok_or_else(|| Error::msg("window missing during resize"))?;
                window.window.resize_terminal(rows, cols, content_rect)?
            };
            self.execute_window_actions_for_window(window_id, actions).await?;
        }

        self.compose_and_send(true).await
    }

    async fn handle_pane_output(&mut self, id: usize, surface: Surface, cursor: Option<(u16, u16)>) -> Result<()> {
        let Some(window_id) = self.window_id_for_pane(id) else {
            return Ok(());
        };
        let is_active = window_id == self.active_window_id;
        let actions = {
            let window = self
                .windows
                .get_mut(&window_id)
                .ok_or_else(|| Error::msg("window missing for pane output"))?;
            window.pane_surfaces.insert(id, surface);
            window.window.handle_pane_output(id, cursor)?
        };
        self.execute_window_actions_for_window(window_id, actions).await?;
        if is_active {
            self.compose_and_send(false).await?;
        }
        Ok(())
    }

    async fn handle_show_widget(&mut self, overlay: OverlayWidget) -> Result<()> {
        self.widget_overlay = Some(overlay);
        self.compose_and_send(true).await
    }

    async fn handle_update_widget(&mut self, overlay: OverlayWidget) -> Result<()> {
        self.widget_overlay = Some(overlay);
        self.compose_and_send(true).await
    }

    async fn handle_hide_widget(&mut self) -> Result<()> {
        self.widget_overlay = None;
        self.compose_and_send(true).await
    }

    async fn handle_get_snapshot(&self, reply_to: oneshot::Sender<RuntimeSessionState>) -> Result<()> {
        reply_to
            .send(self.runtime_snapshot())
            .map_err(|_| Error::msg("failed to send session snapshot"))?;
        Ok(())
    }

    async fn handle_pane_died(&mut self, id: usize) -> Result<()> {
        let Some(window_id) = self.window_id_for_pane(id) else {
            return Ok(());
        };
        let is_active = window_id == self.active_window_id;
        let actions = {
            let window = self
                .windows
                .get_mut(&window_id)
                .ok_or_else(|| Error::msg("window missing for pane death"))?;
            window.pane_handles.remove(&id);
            window.pane_surfaces.remove(&id);
            window.window.remove_pane(id)?
        };
        self.execute_window_actions_for_window(window_id, actions).await?;
        if is_active {
            self.compose_and_send(true).await?;
        }
        Ok(())
    }

    async fn activate_window(&mut self, window_id: u32) -> Result<()> {
        if window_id == self.active_window_id {
            return Ok(());
        }

        let previous_window_id = self.active_window_id;
        self.hide_window_panes(previous_window_id).await?;
        self.active_window_id = window_id;
        self.reveal_window_panes(window_id).await?;
        self.redraw_active_window().await?;
        self.compose_and_send(true).await
    }

    async fn redraw_active_window(&mut self) -> Result<()> {
        let actions = self.active_window_mut()?.window.redraw()?;
        self.execute_window_actions_for_active(actions).await
    }

    async fn hide_window_panes(&self, window_id: u32) -> Result<()> {
        let Some(window) = self.windows.get(&window_id) else {
            return Ok(());
        };
        for pane in window.pane_handles.values() {
            pane.hide().await?;
        }
        Ok(())
    }

    async fn reveal_window_panes(&self, window_id: u32) -> Result<()> {
        let Some(window) = self.windows.get(&window_id) else {
            return Ok(());
        };
        for pane in window.pane_handles.values() {
            pane.reveal().await?;
        }
        Ok(())
    }

    async fn execute_window_actions_for_active(&mut self, actions: Vec<WindowAction>) -> Result<()> {
        self.execute_window_actions_for_window(self.active_window_id, actions)
            .await
    }

    async fn execute_window_actions_for_window(&mut self, window_id: u32, actions: Vec<WindowAction>) -> Result<()> {
        for action in actions {
            match action {
                WindowAction::SendInputToPane { id, bytes } => {
                    if let Some(pane) = self.window_pane_handle(window_id, id) {
                        pane.user_input(bytes).await?;
                    }
                }
                WindowAction::ResizePane { id, rect } => {
                    if let Some(pane) = self.window_pane_handle(window_id, id) {
                        pane.resize(rect).await?;
                    }
                }
                WindowAction::KillPane { id } => {
                    if let Some(pane) = self.window_pane_handle(window_id, id) {
                        pane.kill().await?;
                    }
                }
                WindowAction::RerenderPane { id } => {
                    if let Some(pane) = self.window_pane_handle(window_id, id) {
                        pane.rerender().await?;
                    }
                }
                WindowAction::SpawnPane { id, rect } => {
                    let pane = self.spawn_pane(id, rect)?;
                    if let Some(window) = self.windows.get_mut(&window_id) {
                        window.pane_handles.insert(id, pane);
                    }
                }
            }
        }

        Ok(())
    }

    async fn compose_and_send(&mut self, force: bool) -> Result<()> {
        let bar_state = self.bar_state();
        let active_window = self.active_window()?;
        let mut surface = active_window
            .window
            .compose_surface(&active_window.pane_surfaces, self.config_runtime.pane_style())?;
        for docked in self.docked_layouts() {
            let widget_surface = render_docked_widget(
                docked.rect.width,
                docked.rect.height,
                &docked.kind,
                docked.edge,
                &bar_state,
            );
            surface.overlay_at(&widget_surface, docked.rect.x, docked.rect.y);
        }
        if let Some(overlay) = &self.widget_overlay {
            let overlay_surface = render_overlay_widget(surface.width(), surface.height(), overlay);
            surface.overlay_at(&overlay_surface, 0, 0);
        }
        let output = render_surface_diff(&self.prev_surface, &surface, force);
        self.prev_surface = surface;
        self.session_manager_handle.session_send_output(self.id, output).await
    }

    fn bar_state(&self) -> BarRenderState {
        BarRenderState {
            active_session_name: Some(self.name.clone()),
            windows: self
                .window_order
                .iter()
                .enumerate()
                .filter_map(|(index, window_id)| {
                    self.windows.get(window_id).map(|window| WindowTab {
                        index: index + 1,
                        name: window.name.clone(),
                        is_active: *window_id == self.active_window_id,
                    })
                })
                .collect(),
        }
    }

    fn spawn_pane(&self, id: usize, rect: Rect) -> Result<PaneHandle> {
        Pane::spawn(self.handle.clone(), id, rect)
    }

    fn runtime_snapshot(&self) -> RuntimeSessionState {
        let windows = self
            .window_order
            .iter()
            .enumerate()
            .filter_map(|(index, window_id)| {
                self.windows
                    .get(window_id)
                    .map(|window| self.runtime_window_state(*window_id, index + 1, window))
            })
            .collect::<Vec<_>>();
        let current_window = windows.iter().find(|window| window.is_active).cloned();

        RuntimeSessionState {
            id: self.id,
            name: self.name.clone(),
            is_current: false,
            windows,
            current_window,
        }
    }

    fn runtime_window_state(&self, window_id: u32, index: usize, window: &SessionWindow) -> RuntimeWindowState {
        let panes = window
            .window
            .pane_ids()
            .into_iter()
            .filter_map(|pane_id| {
                window
                    .window
                    .pane_rect(pane_id)
                    .map(|rect| self.runtime_pane_state(&window.window, pane_id, rect))
            })
            .collect::<Vec<_>>();
        let active_pane = panes.iter().find(|pane| pane.is_active).cloned();

        RuntimeWindowState {
            id: window_id,
            index,
            name: window.name.clone(),
            is_active: window_id == self.active_window_id,
            panes,
            active_pane,
        }
    }

    fn runtime_pane_state(&self, window: &Window, pane_id: usize, rect: Rect) -> RuntimePaneState {
        RuntimePaneState {
            id: pane_id,
            is_active: pane_id == window.active_pane_id(),
            rect: RuntimeRectState {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
            },
        }
    }

    fn content_rect(&self) -> Rect {
        Self::content_rect_for(&self.config_runtime, self.terminal_rows, self.terminal_cols)
    }

    fn docked_layouts(&self) -> Vec<DockedWidgetLayout> {
        let root = Rect {
            x: 0,
            y: 0,
            width: self.terminal_cols,
            height: self.terminal_rows,
        };
        let (_, layouts) = compute_docked_layout(root, self.config_runtime.docked_widgets());
        layouts
    }

    fn content_rect_for(config_runtime: &ConfigRuntime, rows: u16, cols: u16) -> Rect {
        let root = Rect {
            x: 0,
            y: 0,
            width: cols,
            height: rows,
        };
        let (content_rect, _) = compute_docked_layout(root, config_runtime.docked_widgets());
        content_rect
    }

    fn active_window(&self) -> Result<&SessionWindow> {
        self.windows
            .get(&self.active_window_id)
            .ok_or_else(|| Error::msg("active window missing"))
    }

    fn active_window_mut(&mut self) -> Result<&mut SessionWindow> {
        self.windows
            .get_mut(&self.active_window_id)
            .ok_or_else(|| Error::msg("active window missing"))
    }

    fn window_pane_handle(&self, window_id: u32, pane_id: usize) -> Option<PaneHandle> {
        self.windows
            .get(&window_id)
            .and_then(|window| window.pane_handles.get(&pane_id))
            .cloned()
    }

    fn window_id_for_pane(&self, pane_id: usize) -> Option<u32> {
        self.windows.iter().find_map(|(window_id, window)| {
            if window.pane_handles.contains_key(&pane_id) || window.window.pane_ids().contains(&pane_id) {
                Some(*window_id)
            } else {
                None
            }
        })
    }

    async fn kill_all_panes(&self) {
        for window in self.windows.values() {
            for (id, pane) in &window.pane_handles {
                if let Err(e) = pane.kill().await {
                    warn!(error=%e, pane_id=*id, session_id=self.id, "Failed to kill pane during session shutdown");
                }
            }
        }
    }
}

use std::{collections::HashMap, mem};

use bytes::Bytes;
use handle_macro::Handle;
use tokio::sync::mpsc;
use tracing::Instrument;

use crate::{
    actors::{
        pane::{Pane, PaneHandle},
        session::SessionHandle,
    }, layout::{LayoutNode, Rect}, prelude::*
};

#[derive(Handle)]
pub enum WindowEvent {
    UserInput { bytes: Bytes },  // input from user
    PaneOutput { id: usize, bytes: Bytes, cursor: Option<(u16, u16)> }, // output from pane
    IteratePane { is_next: bool },
    KillPane,
    Redraw,
    Kill,
}
use WindowEvent::*;

#[allow(unused)]
#[derive(Debug)]
pub enum WindowState {
    Focused,
    Unfocused,
}
#[derive(Debug)]
pub struct Window {
    session_handle: SessionHandle,
    handle: WindowHandle,
    rx: mpsc::Receiver<WindowEvent>,

    layout: LayoutNode,
    layout_sizing_map: HashMap<usize, Rect>,
    panes: HashMap<usize, PaneHandle>,
    pane_cursors: HashMap<usize, (u16, u16)>,
    active_pane_id: usize,
    max_pane_id: usize,
    next_pane_id: usize,

    #[allow(unused)]
    window_state: WindowState,
}
impl Window {
    async fn handle_user_input(&mut self, bytes: Bytes) -> Result<()> {
        if let Some(pane) = self.panes.get(&self.active_pane_id) {
            pane.user_input(bytes).await?;
        }
        Ok(())
    }
    async fn handle_pane_output(&mut self, id: usize, bytes: Bytes, cursor: Option<(u16, u16)>) -> Result<()> {
        if let Some(pos) = cursor {
            self.pane_cursors.insert(id, pos);
        }

        self.session_handle.window_output(bytes).await?;

        if let Some(&(active_x, active_y)) = self.pane_cursors.get(&self.active_pane_id) {
            let restore_cursor = format!("\x1b[{};{}H", active_y, active_x);
            self.session_handle.window_output(Bytes::from(restore_cursor)).await?;
        }
        
        Ok(())
    }
    async fn handle_redraw(&mut self) -> Result<()> {
        for pane in self.panes.values() {
            pane.rerender().await?;
        }
        Ok(())
    }
    async fn handle_iterate_pane(&mut self, is_next: bool) -> Result<()> {
        let mut ids: Vec<usize> = self.panes.keys().copied().collect();
        if ids.is_empty() { return Ok(()); } // Guard against empty window
        ids.sort();

        let current_idx = ids.iter().position(|&id| id == self.active_pane_id).unwrap_or(0);

        let new_idx = if is_next {
            (current_idx + 1) % ids.len()
        } else {
            if current_idx == 0 {
                ids.len() - 1
            } else {
                current_idx - 1
            }
        };

        self.active_pane_id = ids[new_idx];
        debug!("Switched to Pane ID: {}", self.active_pane_id);
        let (tx, ty) = if let Some(&pos) = self.pane_cursors.get(&self.active_pane_id) {
            pos 
        } else {
            if let Some(rect) = self.layout_sizing_map.get(&self.active_pane_id) {
                (rect.x + 1, rect.y + 1)
            } else {
                warn!("Active pane has no rect in layout map!");
                return Ok(());
            }
        };

        let move_cursor = format!("\x1b[{};{}H", ty, tx);
        self.session_handle.window_output(Bytes::from(move_cursor)).await?;

        Ok(())
    }
    async fn handle_kill_pane(&mut self) -> Result<()> {
        let dead_pane_id = self.active_pane_id;
        if self.panes.len() < 1 {
            // TODO: kill window if last pane is killed
            warn!("Can't kill last pane {}", dead_pane_id);
            return Ok(()); 
        }

        debug!("Killing pane {}", dead_pane_id);
        if let Some(pane_handle) = self.panes.get(&dead_pane_id) {
            let _ = pane_handle.kill().await.unwrap();
        }

        self.panes.remove(&dead_pane_id);
        self.pane_cursors.remove(&dead_pane_id);
        self.layout_sizing_map.remove(&dead_pane_id);

        let dummy_node = LayoutNode::Pane { id: 0 }; 
        let old_layout = mem::replace(&mut self.layout, dummy_node);

        if let Some(new_layout) = old_layout.remove_node(dead_pane_id) {
            self.layout = new_layout;
        } else {
            info!("No panes left.");
            return Ok(());
        }

        if let Some(&new_id) = self.panes.keys().next() {
            self.active_pane_id = new_id;
        }

        let root_rect = Rect { x: 0, y: 0, width: 214, height: 62 };
        self.layout_sizing_map.clear();
        self.layout.calculate_layout(root_rect, &mut self.layout_sizing_map)?;

        for (id, pane) in self.panes.iter() {
            if let Some(new_rect) = self.layout_sizing_map.get(id) {
                pane.resize(*new_rect).await?; 
            }
        }

        self.session_handle.window_output(Bytes::from(crossterm::terminal::Clear(crossterm::terminal::ClearType::All).to_string())).await?;
        self.handle_redraw().await?;

        Ok(())
    }
}
impl Window {
    #[instrument(skip(session_handle))]
    pub fn spawn(session_handle: SessionHandle) -> Result<WindowHandle> {
        let window = Window::new(session_handle)?;
        window.run()
    }
    #[instrument(skip(session_handle))]
    fn new(session_handle: SessionHandle) -> Result<Self> {
        let (tx, rx) = mpsc::channel(10);
        let handle = WindowHandle { tx };

        let init_pane_id = 0;
        let mut init_layout_node = LayoutNode::Pane { id: init_pane_id };

        let init_pane_id_1 = 1;

        let mut layout_sizing_map = HashMap::new();
        init_layout_node.add_split(init_pane_id, init_pane_id_1, crate::layout::SplitDirection::Vertical);
        let root_rect = Rect {
            x: 0,
            y: 0,
            width: 214,
            height: 62,
        };
        layout_sizing_map.insert(init_pane_id, root_rect);
        init_layout_node.calculate_layout(root_rect, &mut layout_sizing_map)?;
        let mut panes = HashMap::new();
        if let Some(rect_0) = layout_sizing_map.get(&init_pane_id) {
            let pane_handle = Pane::spawn(handle.clone(), init_pane_id, *rect_0)?;
            panes.insert(init_pane_id, pane_handle);
        }

        if let Some(rect_1) = layout_sizing_map.get(&init_pane_id_1) {
            let pane_handle = Pane::spawn(handle.clone(), init_pane_id_1, *rect_1)?;
            panes.insert(init_pane_id_1, pane_handle);
        }

        Ok(Self {
            session_handle,
            handle,
            rx,
            layout: init_layout_node,
            layout_sizing_map,
            panes,
            active_pane_id: init_pane_id,
            max_pane_id: 1,
            next_pane_id: init_pane_id_1 + 1,
            window_state: WindowState::Focused,
            pane_cursors: HashMap::new(),
        })
    }
    #[instrument(skip(self))]
    fn run(mut self) -> crate::error::Result<WindowHandle> {
        let span = tracing::Span::current();
        let handle_clone = self.handle.clone();
        let _task = tokio::spawn({
            async move {
                loop {
                    if let Some(event) = self.rx.recv().await {
                        match event {
                            UserInput { bytes } => {
                                trace!("Window: UserInput");
                                self.handle_user_input(bytes).await.unwrap();
                            }
                            PaneOutput { id, bytes, cursor } => {
                                trace!("Window: PaneOutput");
                                self.handle_pane_output(id, bytes, cursor).await.unwrap();
                            }
                            IteratePane { is_next } => {
                                debug!("Window: IteratePane");
                                self.handle_iterate_pane(is_next).await.unwrap();
                            }
                            KillPane => {
                                debug!("Window: IteratePane");
                                self.handle_kill_pane().await.unwrap();
                            }
                            Redraw => {
                                debug!("Window: Redraw");
                                self.handle_redraw().await.unwrap();
                            }
                            Kill => {
                                debug!("Window: Kill");
                                for pane in self.panes.values() {
                                    pane.kill().await.unwrap();
                                }
                                break;
                            }
                        }
                    }
                }
            }
            .instrument(span)
        });

        Ok(handle_clone)
    }
}

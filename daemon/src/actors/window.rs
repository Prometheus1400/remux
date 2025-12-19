use std::{collections::BTreeMap, mem};

use bytes::Bytes;
use handle_macro::Handle;
use tokio::sync::mpsc;
use tracing::Instrument;

use crate::{
    actors::{
        pane::{Pane, PaneHandle},
        session::SessionHandle,
    }, cell::set_cursor_position, layout::{LayoutNode, Rect, SplitDirection}, prelude::*
};

#[derive(Handle)]
pub enum WindowEvent {
    UserInput(Bytes), // input from user
    PaneOutput {
        id: usize,
        bytes: Bytes,
        cursor: Option<(u16, u16)>,
    }, // output from pane
    IteratePane {
        is_next: bool,
    },
    SplitPane {
        direction: SplitDirection,
    },
    KillPane,
    Redraw,
    TerminalResize {
        rows: u16,
        cols: u16,
    },
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
    layout_sizing_map: BTreeMap<usize, Rect>,
    panes: BTreeMap<usize, PaneHandle>,
    pane_cursors: BTreeMap<usize, (u16, u16)>,
    active_pane_id: usize,
    next_pane_id: usize,
    root_rect: Rect,

    #[allow(unused)]
    window_state: WindowState,
}
impl Window {
    #[instrument(skip(session_handle), name = "Window")]
    pub fn spawn(session_handle: SessionHandle) -> Result<WindowHandle> {
        let window = Window::new(session_handle)?;
        window.run()
    }

    fn new(session_handle: SessionHandle) -> Result<Self> {
        let (tx, rx) = mpsc::channel(10);
        let handle = WindowHandle { tx };

        let init_pane_id = 0;
        let init_layout_node = LayoutNode::Pane { id: init_pane_id };

        // Default size, overridden when client connects and send new size
        let (cols, rows) = (80, 24);
        let root_rect = Rect {
            x: 0,
            y: 0,
            width: cols,
            height: rows,
        };
        let mut layout_sizing_map = BTreeMap::new();
        layout_sizing_map.insert(init_pane_id, root_rect);
        init_layout_node.calculate_layout(root_rect, &mut layout_sizing_map)?;

        let mut panes = BTreeMap::new();
        if let Some(rect) = layout_sizing_map.get(&init_pane_id) {
            let pane_handle = Pane::spawn(handle.clone(), init_pane_id, *rect)?;
            panes.insert(init_pane_id, pane_handle);
        }

        Ok(Self {
            session_handle,
            handle,
            rx,
            layout: init_layout_node,
            layout_sizing_map,
            panes,
            active_pane_id: init_pane_id,
            next_pane_id: init_pane_id + 1,
            window_state: WindowState::Focused,
            pane_cursors: BTreeMap::new(),
            root_rect,
        })
    }
    #[instrument(skip(self))]
    fn run(mut self) -> Result<WindowHandle> {
        let handle_clone = self.handle.clone();
        let _task = tokio::spawn({
            async move {
                loop {
                    if let Some(event) = self.rx.recv().await {
                        match event {
                            UserInput(bytes) => {
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
                            SplitPane { direction } => {
                                debug!("Window: SplitPane");
                                self.handle_split_pane(direction).await.unwrap();
                            }
                            KillPane => {
                                debug!("Window: KillPane");
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
                            TerminalResize { rows, cols } => {
                                debug!("Window: TerminalResize");
                                self.handle_terminal_resize(rows, cols).await.unwrap();
                            }
                        }
                    }
                }
            }
            .in_current_span()
        });

        Ok(handle_clone)
    }
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
        self.draw_pane_borders().await?;

        for pane in self.panes.iter() {
            pane.1.rerender().await?;
        }
        Ok(())
    }
    async fn handle_iterate_pane(&mut self, is_next: bool) -> Result<()> {
        let ids: Vec<usize> = self.panes.keys().copied().collect();
        if ids.is_empty() {
            return Ok(());
        }
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

        self.draw_pane_borders().await?;

        let move_cursor = format!("\x1b[{};{}H", ty, tx);
        self.session_handle.window_output(Bytes::from(move_cursor)).await?;

        Ok(())
    }
    async fn handle_split_pane(&mut self, direction: SplitDirection) -> Result<()> {
        self.layout.add_split(self.active_pane_id, self.next_pane_id, direction);
        self.layout
            .calculate_layout(self.root_rect, &mut self.layout_sizing_map)?;

        // new pane rect
        if let Some(rect) = self.layout_sizing_map.get(&self.next_pane_id) {
            let pane_handle = Pane::spawn(self.handle.clone(), self.next_pane_id, *rect)?;
            self.panes.insert(self.next_pane_id, pane_handle);
        }

        self.active_pane_id = self.next_pane_id;
        self.next_pane_id += 1;

        for (id, pane) in self.panes.iter() {
            if let Some(new_rect) = self.layout_sizing_map.get(id) {
                pane.resize(*new_rect).await?;
            }
        }

        self.handle_redraw().await?;
        Ok(())
    }
    async fn handle_kill_pane(&mut self) -> Result<()> {
        let dead_pane_id = self.active_pane_id;
        if self.panes.len() <= 1 {
            // TODO: kill window if last pane is killed
            warn!("Can't kill last pane {}", dead_pane_id);
            return Ok(());
        }

        debug!("Killing pane {}", dead_pane_id);
        if let Some(pane_handle) = self.panes.get(&dead_pane_id) {
            if let Err(e) = pane_handle.kill().await {
                error!("Error while killing pane! {}", e);
            }
        }

        self.panes.remove(&dead_pane_id);
        self.pane_cursors.remove(&dead_pane_id);

        let dummy_node = LayoutNode::Pane { id: 0 };
        let old_layout = mem::replace(&mut self.layout, dummy_node);

        if let Some(new_layout) = old_layout.remove_node(dead_pane_id) {
            self.layout = new_layout;
        } else {
            info!("No panes left.");
            return Ok(());
        }

        self.handle_iterate_pane(false).await?;

        self.layout_sizing_map.clear();
        self.layout
            .calculate_layout(self.root_rect, &mut self.layout_sizing_map)?;

        for (id, pane) in self.panes.iter() {
            if let Some(new_rect) = self.layout_sizing_map.get(id) {
                pane.resize(*new_rect).await?;
            }
        }

        self.handle_redraw().await?;
        Ok(())
    }
    async fn handle_terminal_resize(&mut self, rows: u16, cols: u16) -> Result<()> {
        self.root_rect = Rect {
            x: 0,
            y: 0,
            width: cols,
            height: rows,
        };

        self.layout
            .calculate_layout(self.root_rect, &mut self.layout_sizing_map)?;

        for (id, pane) in self.panes.iter() {
            if let Some(new_rect) = self.layout_sizing_map.get(id) {
                pane.resize(*new_rect).await?;
            }
        }

        self.handle_redraw().await?;
        Ok(())
    }

    async fn draw_pane_borders(&mut self) -> Result<()> {
        let cols = self.root_rect.width;
        let rows = self.root_rect.height;
        
        let mut output_buffer = Vec::with_capacity(cols as usize * rows as usize * 4);

        // grab active pane rectangle
        let active_rect = self.layout_sizing_map.get(&self.active_pane_id);

        // checks if cell is in a pane or not
        let is_content = |x: u16, y: u16, map: &BTreeMap<usize, Rect>| -> bool {
            for rect in map.values() {
                if x >= rect.x && x < rect.x + rect.width && 
                y >= rect.y && y < rect.y + rect.height {
                    return true;
                }
            }
            false
        };

        // reset colors
        output_buffer.extend_from_slice(b"\x1b[0m"); 

        // set initial cursor position
        let mut cursor_row = 0;
        let mut cursor_col = 0;
        let mut cursor_invalid = true;

        for y in 0..rows {
            for x in 0..cols {
                // skip cells in panes
                if is_content(x, y, &self.layout_sizing_map) {
                    continue;
                }

                // get surrounding borders
                let north = y > 0 && !is_content(x, y - 1, &self.layout_sizing_map);
                let south = y < rows - 1 && !is_content(x, y + 1, &self.layout_sizing_map);
                let west  = x > 0 && !is_content(x - 1, y, &self.layout_sizing_map);
                let east  = x < cols - 1 && !is_content(x + 1, y, &self.layout_sizing_map);

                // pattern match to get correct border char
                let border_char = match (north, south, east, west) {
                    (true,  true,  false, false) => '│',
                    (false, false, true,  true)  => '─',
                    (false, true,  true,  false) => '┌',
                    (false, true,  false, true)  => '┐',
                    (true,  false, true,  false) => '└',
                    (true,  false, false, true)  => '┘',
                    (true,  true,  true,  false) => '├',
                    (true,  true,  false, true)  => '┤',
                    (false, true,  true,  true)  => '┬',
                    (true,  false, true,  true)  => '┴',
                    (true,  true,  true,  true)  => '┼',
                    (true,  false, false, false) => '│', 
                    (false, true,  false, false) => '│', 
                    (false, false, true,  false) => '─', 
                    (false, false, false, true)  => '─', 
                    _ => ' ',
                };

                // move cursor if at the wrong spot
                if cursor_invalid || y != cursor_row || x != cursor_col {
                    set_cursor_position(&mut output_buffer, x + 1, y + 1);
                    cursor_invalid = false;
                    cursor_row = y;
                    cursor_col = x;
                }

                // if the border is on the active pane, set this to true
                let mut is_active_border = false;
                if let Some(rect) = active_rect {
                    if x >= rect.x.saturating_sub(1) && x < rect.x + rect.width + 1 &&
                       y >= rect.y.saturating_sub(1) && y < rect.y + rect.height + 1 {
                        is_active_border = true;
                    }
                }

                // configurable colors later
                if is_active_border {
                    output_buffer.extend_from_slice(b"\x1b[96m");
                } else {
                    output_buffer.extend_from_slice(b"\x1b[90m");
                }

                // add ANSI
                let mut border_char_buf = [0u8; 4]; 
                let str_slice = border_char.encode_utf8(&mut border_char_buf);
                output_buffer.extend_from_slice(str_slice.as_bytes());

                cursor_col += 1;
            }
        }

        // send to session
        if !output_buffer.is_empty() {
            self.session_handle.window_output(Bytes::from(output_buffer)).await?;
        }
        
        Ok(())
    }
}

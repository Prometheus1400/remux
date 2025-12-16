use std::time::Duration;

use bytes::Bytes;
use handle_macro::Handle;
use tokio::{sync::mpsc, time::MissedTickBehavior};
use tracing::Instrument;

use crate::{
    actors::{
        pty::{Pty, PtyHandle},
        window::WindowHandle,
    },
    cell::RemuxCell,
    layout::Rect,
    prelude::*,
};

#[derive(Handle, Debug)]
pub enum PaneEvent {
    UserInput(Bytes),
    PtyOutput(Bytes),
    Render,   // uses the diff from prev state to get to desired state (falls back to rerender if no prev state)
    Rerender, // full rerender
    Resize { rect: Rect },
    Hide,
    Reveal,
    Kill,
}
use PaneEvent::*;

pub enum PaneState {
    Visible,
    Hidden,
}

pub struct Pane {
    id: usize,
    handle: PaneHandle,
    window_handle: WindowHandle,
    rx: mpsc::Receiver<PaneEvent>,
    pane_state: PaneState,
    pty_handle: PtyHandle,

    // cells
    force_rerender: bool,
    curr_grid: Vec<RemuxCell>,
    prev_grid: Vec<RemuxCell>,

    // vte related
    vte: vt100::Parser,
    rect: Rect,
}
impl Pane {
    #[instrument(skip(window_handle, rect), name = "Pane")]
    pub fn spawn(window_handle: WindowHandle, id: usize, rect: Rect) -> Result<PaneHandle> {
        let pane = Pane::new(window_handle, id, rect)?;
        pane.run()
    }
    fn new(window_handle: WindowHandle, id: usize, rect: Rect) -> Result<Self> {
        let (tx, rx) = mpsc::channel(10);
        let handle = PaneHandle { tx };

        let total_cells = (rect.width * rect.height) as usize;
        let curr_grid = vec![RemuxCell::default(); total_cells];
        let prev_grid = vec![RemuxCell::default(); total_cells];

        let vte = vt100::Parser::new(rect.height, rect.width, 0);
        let pty_handle = Pty::spawn(handle.clone(), rect)?;
        Ok(Self {
            id,
            handle,
            window_handle,
            pty_handle,
            rx,
            force_rerender: true,
            curr_grid,
            prev_grid,
            vte,
            pane_state: PaneState::Visible,
            rect,
        })
    }
    fn run(mut self) -> Result<PaneHandle> {
        let handle_clone = self.handle.clone();

        let mut render_ticker = tokio::time::interval(Duration::from_millis(16)); // 60 fps
        render_ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let mut is_dirty = true;
        let _task = tokio::spawn(
            async move {
                loop {
                    tokio::select! {
                        _ = render_ticker.tick() => {
                            if let PaneState::Visible = self.pane_state {
                                if self.force_rerender || is_dirty {
                                    if let Err(e) = self.handle_render().await {
                                        error!("Failed to render frame: {e}")
                                    }
                                    is_dirty = false;
                                }
                            }
                        }
                        event_result = self.rx.recv() => {
                            match event_result {
                                Some(event) => {
                                    match &event {
                                        UserInput(..) | PtyOutput(..) => {
                                            trace!(event=?event);
                                        }
                                        _ => {
                                            info!(event=?event);
                                        }
                                    }
                                    match event {
                                        UserInput(bytes) => {
                                            self.handle_input(bytes).await.unwrap();
                                        }
                                        PtyOutput(bytes) => {
                                            if let Err(e) = self.handle_pty_output(bytes).await {
                                                error!("Error while handling PTY output: {}", e);
                                            }
                                            is_dirty = true;
                                        }
                                        Kill => {
                                            self.pty_handle.kill().await.unwrap();
                                            debug!("Pty died");
                                            break;
                                        }
                                        Render => {
                                            is_dirty = true;
                                        }
                                        Rerender => {
                                            self.force_rerender = true;
                                        }
                                        Resize { rect } => {
                                            self.handle_resize(rect).await.unwrap();
                                            self.force_rerender = true;
                                        }
                                        Hide => {
                                            self.pane_state = PaneState::Hidden;
                                        }
                                        Reveal => {
                                            self.pane_state = PaneState::Visible;
                                        }
                                    }
                                }
                                None => {
                                    error!("Channel closed");
                                    break;
                                }
                            }
                        }
                    }
                }
            }
                .in_current_span(),
        );

        Ok(handle_clone)
    }

    async fn handle_input(&mut self, bytes: Bytes) -> Result<()> {
        self.pty_handle.input(bytes).await.unwrap();
        Ok(())
    }

    async fn handle_pty_output(&mut self, bytes: Bytes) -> Result<()> {
        self.vte.process(&bytes);
        Ok(())
    }

    async fn handle_render(&mut self) -> Result<()> {
        let screen = self.vte.screen();
        let rows = self.rect.height as usize;
        let cols = self.rect.width as usize;

        let total_cells = rows * cols;
        if self.curr_grid.len() != total_cells {
            self.curr_grid.resize(total_cells, RemuxCell::default());
        }
        if self.prev_grid.len() != total_cells {
            self.prev_grid.resize(total_cells, RemuxCell::default());
        }

        for r in 0..rows {
            for c in 0..cols {
                let idx = r * cols + c;
                let remux_cell = &mut self.curr_grid[idx];
                if let Some(cell) = screen.cell(r as u16, c as u16) {
                    remux_cell.set_content(cell.contents().as_bytes());
                    remux_cell.set_fg_color(cell.fgcolor());
                    remux_cell.set_bg_color(cell.bgcolor());
                    remux_cell.set_attributes_from_vt100(cell);
                } else {
                    *remux_cell = RemuxCell::default();
                }
            }
        }

        let output = RemuxCell::render_diff(self.rect, &self.prev_grid, &self.curr_grid, self.force_rerender);
        std::mem::swap(&mut self.prev_grid, &mut self.curr_grid);
        self.force_rerender = false;

        let (c_row, c_col) = screen.cursor_position();
        let global_x = self.rect.x + 1 + c_col;
        let global_y = self.rect.y + 1 + c_row;

        self.window_handle
            .pane_output(self.id, Bytes::from(output), Some((global_x, global_y)))
            .await
    }

    async fn handle_resize(&mut self, rect: Rect) -> Result<()> {
        self.rect = rect;

        let new_size = (rect.width * rect.height) as usize;
        self.prev_grid.resize(new_size, RemuxCell::default());
        self.curr_grid.resize(new_size, RemuxCell::default());

        self.pty_handle.resize(rect).await?;
        self.vte.set_size(rect.height, rect.width);
        Ok(())
    }
}

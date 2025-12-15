use bytes::Bytes;
use handle_macro::Handle;
use tokio::sync::mpsc;
use tracing::Instrument;

use crate::{
    actors::{
        pty::{Pty, PtyHandle},
        window::WindowHandle,
    }, cell::{CONTENT_LENGTH, RemuxCell}, layout::Rect, prelude::*
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
    curr_grid: Vec<Vec<RemuxCell>>,

    // vte related
    vte: vt100::Parser,
    prev_screen_state: Option<vt100::Screen>,
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

        let curr_grid = vec![vec![RemuxCell::default(); rect.width as usize]; rect.height as usize];
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
            vte,
            pane_state: PaneState::Visible,
            prev_screen_state: None,
            rect,
        })
    }
    fn run(mut self) -> Result<PaneHandle> {
        let handle_clone = self.handle.clone();
        let _task = tokio::spawn(
            async move {
                loop {
                    if let Some(event) = self.rx.recv().await {
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
                            }
                            Kill => {
                                self.pty_handle.kill().await.unwrap();
                                debug!("Pty died");
                                break;
                            }
                            Render => {
                                self.handle_render().await.unwrap();
                            }
                            Rerender => {
                                self.handle_rerender().await.unwrap();
                            }
                            Resize { rect } => {
                                self.handle_resize(rect).await.unwrap();
                            }
                            Hide => {
                                self.pane_state = PaneState::Hidden;
                            }
                            Reveal => {
                                self.pane_state = PaneState::Visible;
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
        self.handle_rerender().await
    }

    // TODO: below code is bad and unused, need better diffing solution
    async fn handle_render(&mut self) -> Result<()> {
        match &self.prev_screen_state {
            Some(prev) => {
                let cur_screen_state = self.vte.screen();
                let diff = cur_screen_state.state_diff(prev);
                self.prev_screen_state = Some(cur_screen_state.clone());
                let (c_row, c_col) = cur_screen_state.cursor_position();
                let global_x = self.rect.x + 1 + c_col;
                let global_y = self.rect.y + 1 + c_row;
                Ok(self
                    .window_handle
                    .pane_output(self.id, Bytes::copy_from_slice(&diff), Some((global_x, global_y)))
                    .await?)
            }
            None => self.handle_rerender().await,
        }
    }

    // async fn handle_rerender(&mut self) -> Result<()> {
    //     let screen = self.vte.screen();
    //
    //     self.prev_screen_state = Some(screen.clone());
    //     let mut output = Vec::new();
    //
    //     for (i, row) in screen.rows_formatted(0, self.rect.width).enumerate() {
    //         let cx = self.rect.x + 1;
    //         let cy = self.rect.y + 1 + (i as u16);
    //
    //         let move_cursor = format!("\x1b[{};{}H", cy, cx);
    //         output.extend_from_slice(move_cursor.as_bytes());
    //
    //         let erase_chars = format!("\x1b[{}X", self.rect.width);
    //         output.extend_from_slice(erase_chars.as_bytes());
    //         output.extend_from_slice(&row);
    //     }
    //
    //     output.extend_from_slice(b"\x1b[0m");
    //
    //     let (c_row, c_col) = screen.cursor_position();
    //     let global_x = self.rect.x + 1 + c_col;
    //     let global_y = self.rect.y + 1 + c_row;
    //
    //     self.window_handle
    //         .pane_output(self.id, Bytes::from(output), Some((global_x, global_y)))
    //         .await
    // }


    async fn handle_rerender(&mut self) -> Result<()> {
        let screen = self.vte.screen();

        let rows = self.rect.height as usize;
        let cols = self.rect.width as usize;

        let mut new_grid: Vec<Vec<RemuxCell>> = vec![vec![RemuxCell::default(); cols]; rows];

        for r in 0..rows {
            for c in 0..cols {
                if let Some(cell) = screen.cell(r as u16, c as u16) {
                    let content_str = cell.contents();
                    let bytes = content_str.as_bytes();

                    let mut content_buf = [0u8; CONTENT_LENGTH];
                    if bytes.is_empty() || bytes[0] == 0 {
                        content_buf[0] = b' ';
                    } else {
                        let len = bytes.len().min(CONTENT_LENGTH);
                        content_buf[..len].copy_from_slice(&bytes[..len]);
                    }
                    new_grid[r][c] = RemuxCell {
                        contents: content_buf,
                        fg_color: cell.fgcolor(),
                        bg_color: cell.bgcolor(),
                        bold: cell.bold(),
                        italic: cell.italic(),
                        underline: cell.underline(),
                        is_wide: cell.is_wide(),
                        is_wide_spacer: cell.is_wide_continuation()
                    }
                }
            }
        }


        let output = RemuxCell::render_diff(self.rect, &self.curr_grid, &new_grid, self.force_rerender);
        self.curr_grid = new_grid;
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
        self.pty_handle.resize(rect).await?;
        self.vte.set_size(rect.height, rect.width);

        self.force_rerender = true;
        Ok(())
    }
}

use bytes::Bytes;
use handle_macro::Handle;
use tokio::sync::mpsc;
use tracing::Instrument;

use crate::{
    actors::{
        pty::{Pty, PtyHandle},
        window::WindowHandle,
    },
    control_signals::CLEAR,
    prelude::*,
};

#[derive(Handle)]
pub enum PaneEvent {
    UserInput { bytes: Bytes },
    PtyOutput { bytes: Bytes },
    PtyDied,
    Render, // uses the diff from prev state to get to desired state (falls back to rerender if no prev state)
    Rerender, // full rerender
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
    handle: PaneHandle,
    window_handle: WindowHandle,
    rx: mpsc::Receiver<PaneEvent>,
    pane_state: PaneState,
    pty_handle: PtyHandle,
    // vte related
    vte: vt100::Parser,
    prev_screen_state: Option<vt100::Screen>,
}
impl Pane {
    #[instrument(skip(window_handle))]
    pub fn spawn(window_handle: WindowHandle) -> Result<PaneHandle> {
        let pane = Pane::new(window_handle)?;
        pane.run()
    }
    #[instrument(skip(window_handle))]
    fn new(window_handle: WindowHandle) -> Result<Self> {
        let (tx, rx) = mpsc::channel(10);
        let handle = PaneHandle { tx };
        let vte = vt100::Parser::default();
        let pty_handle = Pty::spawn(handle.clone())?;
        Ok(Self {
            handle,
            window_handle,
            pty_handle,
            rx,
            vte,
            pane_state: PaneState::Visible,
            prev_screen_state: None,
        })
    }
    #[instrument(skip(self))]
    fn run(mut self) -> Result<PaneHandle> {
        let span = tracing::Span::current();
        let handle_clone = self.handle.clone();
        let _task = tokio::spawn({
            async move {
                loop {
                    if let Some(event) = self.rx.recv().await {
                        match event {
                            UserInput { bytes } => {
                                trace!("Pane: UserInput({bytes:?})");
                                self.handle_input(bytes).await.unwrap();
                            }
                            PtyOutput { bytes } => {
                                trace!("Pane: PtyOutput({bytes:?}");
                                self.handle_pty_output(bytes).await.unwrap();
                            }
                            PtyDied => {
                                debug!("Pane: PtyDied");
                                // TODO: notify the window that pane has died
                                break;
                            }
                            Kill => {
                                debug!("Pane: Kill");
                                self.pty_handle.kill().await.unwrap();
                                break;
                            }
                            Render => {
                                trace!("Pane: Render");
                                self.handle_render().await.unwrap();
                            }
                            Rerender => {
                                debug!("Pane: Rerender");
                                self.handle_rerender().await.unwrap();
                            }
                            Hide => {
                                debug!("Pane: Hide");
                                self.pane_state = PaneState::Hidden;
                            }
                            Reveal => {
                                debug!("Pane: Reveal");
                                self.pane_state = PaneState::Visible;
                            }
                        }
                    }
                }
            }
            .instrument(span)
        });

        Ok(handle_clone)
    }

    async fn handle_input(&mut self, bytes: Bytes) -> Result<()> {
        self.pty_handle.input(bytes).await.unwrap();
        Ok(())
    }

    async fn handle_pty_output(&mut self, bytes: Bytes) -> Result<()> {
        self.vte.process(&bytes);
        self.handle.render().await
    }

    async fn handle_render(&mut self) -> Result<()> {
        match &self.prev_screen_state {
            Some(prev) => {
                let cur_screen_state = self.vte.screen().clone();
                let diff = cur_screen_state.state_diff(prev);
                self.prev_screen_state = Some(cur_screen_state);
                Ok(self
                    .window_handle
                    .pane_output(Bytes::copy_from_slice(&diff))
                    .await?)
            }
            None => self.handle_rerender().await,
        }
    }

    async fn handle_rerender(&mut self) -> Result<()> {
        let cur_screen_state = self.vte.screen().clone();
        self.prev_screen_state = Some(cur_screen_state);

        let new_state = self.vte.screen().state_formatted();
        let output = CLEAR.iter().chain(new_state.iter()).copied().collect();
        self.window_handle.pane_output(output).await
        // TODO : todo!("don't need to send this when pane is hidden");
        // match self.state {
        //     PaneState::Visible => {
        //     },
        //     PaneState::Hidden => {
        //     }
        // }
    }
}

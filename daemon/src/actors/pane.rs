use bytes::Bytes;
use tokio::sync::mpsc;

use crate::{
    actors::{
        pty::{Pty, PtyHandle},
        window::WindowHandle,
    },
    control_signals::CLEAR,
    prelude::*,
};

pub enum PaneEvent {
    UserInput(Bytes),
    PtyOutput(Bytes),
    PtyDied,
    Render, // uses the diff from prev state to get to desired state (falls back to rerender if no prev state)
    Rerender, // full rerender
    Hide,
    Reveal,
    Kill,
}

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
    pub fn spawn(window_handle: WindowHandle) -> Result<PaneHandle> {
        let pane = Pane::new(window_handle)?;
        pane.run()
    }
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
    fn run(mut self) -> Result<PaneHandle> {
        let handle_clone = self.handle.clone();
        let _task = tokio::spawn({
            async move {
                loop {
                    if let Some(event) = self.rx.recv().await {
                        use PaneEvent::*;
                        match event {
                            UserInput(bytes) => {
                                trace!("Pane: UserInput");
                                self.handle_input(bytes).await.unwrap();
                            }
                            PtyOutput(bytes) => {
                                trace!("Pane: PtyOutput");
                                self.handle_pty_output(bytes).await.unwrap();
                            }
                            PtyDied => {
                                trace!("Pane: PtyDied");
                                // TODO: notify the window that pane has died
                                break;
                            }
                            Kill => {
                                trace!("Pane: Kill");
                                self.pty_handle.kill().await.unwrap();
                                break;
                            }
                            Render => {
                                trace!("Pane: Render");
                                self.handle_render().await.unwrap();
                            }
                            Rerender => {
                                trace!("Pane: Rerender");
                                self.handle_rerender().await.unwrap();
                            }
                            Hide => {
                                trace!("Pane: Hide");
                                self.pane_state = PaneState::Hidden;
                            }
                            Reveal => {
                                trace!("Pane: Reveal");
                                self.pane_state = PaneState::Visible;
                            }
                        }
                    }
                }
            }
        });

        Ok(handle_clone)
    }

    async fn handle_input(&mut self, bytes: Bytes) -> Result<()> {
        self.pty_handle.send(bytes).await.unwrap();
        Ok(())
    }

    async fn handle_pty_output(&mut self, bytes: Bytes) -> Result<()> {
        self.vte.process(&bytes);
        self.handle.request_render().await
    }

    async fn handle_render(&mut self) -> Result<()> {
        match &self.prev_screen_state {
            Some(prev) => {
                let cur_screen_state = self.vte.screen().clone();
                let diff = cur_screen_state.state_diff(prev);
                self.prev_screen_state = Some(cur_screen_state);
                Ok(self
                    .window_handle
                    .send_pane_output(Bytes::copy_from_slice(&diff))
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
        self.window_handle.send_pane_output(output).await
        // TODO : todo!("don't need to send this when pane is hidden");
        // match self.state {
        //     PaneState::Visible => {
        //     },
        //     PaneState::Hidden => {
        //     }
        // }
    }
}

#[derive(Debug, Clone)]
pub struct PaneHandle {
    tx: mpsc::Sender<PaneEvent>,
}
#[allow(unused)]
impl PaneHandle {
    // public api
    pub async fn send_user_input(&self, bytes: Bytes) -> Result<()> {
        Ok(self.tx.send(PaneEvent::UserInput(bytes)).await?)
    }
    pub async fn send_output_from_pty(&self, bytes: Bytes) -> Result<()> {
        Ok(self.tx.send(PaneEvent::PtyOutput(bytes)).await?)
    }
    pub async fn request_rerender(&self) -> Result<()> {
        Ok(self.tx.send(PaneEvent::Rerender).await?)
    }
    pub async fn notify_pty_died(&self) -> Result<()> {
        Ok(self.tx.send(PaneEvent::PtyDied).await?)
    }
    pub async fn hide(&self) -> Result<()> {
        Ok(self.tx.send(PaneEvent::Hide).await?)
    }
    pub async fn reveal(&self) -> Result<()> {
        Ok(self.tx.send(PaneEvent::Reveal).await?)
    }
    pub async fn kill(&self) -> Result<()> {
        Ok(self.tx.send(PaneEvent::Kill).await?)
    }
    // for internal use only
    async fn request_render(&self) -> Result<()> {
        Ok(self.tx.send(PaneEvent::Render).await?)
    }
}

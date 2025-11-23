use std::collections::HashMap;

use bytes::Bytes;
use handle_macro::Handle;
use tokio::sync::mpsc;
use tracing::Instrument;

use crate::{
    actors::{
        pane::{Pane, PaneHandle},
        session::SessionHandle,
    },
    prelude::*,
};

#[derive(Handle)]
pub enum WindowEvent {
    UserInput { bytes: Bytes },  // input from user
    PaneOutput { bytes: Bytes }, // output from pane
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

    panes: HashMap<usize, PaneHandle>,
    active_pane_id: usize,
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
    async fn handle_pane_output(&mut self, bytes: Bytes) -> Result<()> {
        self.session_handle.window_output(bytes).await
    }
    async fn handle_redraw(&mut self) -> Result<()> {
        if let Some(pane) = self.panes.get(&self.active_pane_id) {
            pane.rerender().await?;
        }
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

        let init_pane_id = 1;
        let pane_handle = Pane::spawn(handle.clone(), init_pane_id)?;
        let mut panes = HashMap::new();
        panes.insert(init_pane_id, pane_handle);
        Ok(Self {
            session_handle,
            handle,
            rx,
            panes,
            active_pane_id: init_pane_id,
            next_pane_id: init_pane_id + 1,
            window_state: WindowState::Focused,
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
                            PaneOutput { bytes } => {
                                trace!("Window: PaneOutput");
                                self.handle_pane_output(bytes).await.unwrap();
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

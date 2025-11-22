use bytes::Bytes;
use tokio::sync::mpsc;
use tracing::Instrument;

use crate::{
    actors::{
        pane::{Pane, PaneHandle},
        session::SessionHandle,
    },
    prelude::*,
};

#[derive(Debug)]
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
    pane_handle: PaneHandle, // TODO: handle more panes
    #[allow(unused)]
    window_state: WindowState,
}
impl Window {
    async fn handle_user_input(&mut self, bytes: Bytes) -> Result<()> {
        self.pane_handle.send_user_input(bytes).await
    }
    async fn handle_pane_output(&mut self, bytes: Bytes) -> Result<()> {
        self.session_handle.send_window_output(bytes).await
    }
    async fn handle_redraw(&mut self) -> Result<()> {
        self.pane_handle.request_rerender().await
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
        let pane_handle = Pane::spawn(handle.clone())?;
        Ok(Self {
            session_handle,
            handle,
            rx,
            pane_handle,
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
                                self.pane_handle.kill().await.unwrap();
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
#[derive(Debug, Clone)]
pub struct WindowHandle {
    tx: mpsc::Sender<WindowEvent>,
}
impl WindowHandle {
    handle_method!(send_pane_output, PaneOutput, bytes: Bytes);
    handle_method!(send_user_input, UserInput, bytes: Bytes);
    handle_method!(redraw, Redraw);
    handle_method!(kill, Kill);
}

use bytes::Bytes;
use tokio::sync::mpsc;

use crate::{
    actor::{Actor, ActorHandle},
    pane::{Pane, PaneHandle}, session::SessionHandle,
};
use tracing::trace;

#[derive(Debug)]
pub enum WindowEvent {
    UserInput(Bytes),  // input from user
    PaneOutput(Bytes), // output from pane
    Redraw,
    Kill,
}
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
    window_state: WindowState,
}
impl Window {
    async fn handle_user_input(&mut self, bytes: Bytes) {
        self.pane_handle.send_user_input(bytes).await.unwrap()
    }
    async fn handle_pane_output(&mut self, bytes: Bytes) {
        self.session_handle.send_window_output(bytes).await;
    }
    async fn handle_redraw(&mut self) {
        self.pane_handle.rerender().await.unwrap()
    }
}
impl Window {
    pub fn new(session_handle: SessionHandle) -> Self {
        let (tx, rx) = mpsc::channel(10);
        let handle = WindowHandle { tx };
        let pane_handle = Pane::new(handle.clone()).run().unwrap();
        Self {
            session_handle,
            handle,
            rx,
            pane_handle,
            window_state: WindowState::Focused,
        }
    }
}
impl Actor<WindowHandle> for Window {
    fn run(mut self) -> crate::error::Result<WindowHandle> {
        let handle_clone = self.handle.clone();
        let _task = tokio::spawn({
            async move {
                loop {
                    tokio::select! {
                        Some(event) = self.rx.recv() => {
                            use WindowEvent::*;
                            match event {
                                UserInput(bytes) => {
                                    trace!("Window: UserInput");
                                    self.handle_user_input(bytes).await;
                                },
                                PaneOutput(bytes) => {
                                    trace!("Window: PaneOutput");
                                    self.handle_pane_output(bytes).await;
                                }
                                Redraw => {
                                    trace!("Window: Redraw");
                                    
                                }
                                Kill => {
                                    trace!("Window: Kill");
                                    self.pane_handle.kill().await.unwrap();
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        });

        Ok(handle_clone)
    }
}
#[derive(Debug, Clone)]
pub struct WindowHandle {
    tx: mpsc::Sender<WindowEvent>,
}
impl WindowHandle {
    pub async fn send_pane_output(&self, bytes: Bytes) {
        self.tx
            .send(WindowEvent::PaneOutput(bytes))
            .await
            .unwrap();
    }
    pub async fn send_user_input(&self, bytes: Bytes) {
        self.tx
            .send(WindowEvent::UserInput(bytes))
            .await
            .unwrap();
    }

    pub async fn redraw(&self) {
        self.tx.send(WindowEvent::Redraw).await.unwrap();
    }
}
impl ActorHandle for WindowHandle {
    async fn kill(&self) -> crate::error::Result<()> {
        self.tx.send(WindowEvent::Kill).await.unwrap();
        Ok(())
    }

    fn is_alive(&self) -> bool {
        !self.tx.is_closed()
    }
}

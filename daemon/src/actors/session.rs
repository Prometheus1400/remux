use bytes::Bytes;
use tokio::sync::mpsc;

use crate::{
    actors::{
        session_manager::SessionManagerHandle,
        window::{Window, WindowHandle},
    },
    error::Result,
    prelude::*,
};

#[derive(Debug)]
pub enum SessionEvent {
    UserInput(Bytes),
    WindowOutput(Bytes),
    NewConnection,
    Kill,
}

pub struct Session {
    id: u32,
    handle: SessionHandle,
    session_manager_handle: SessionManagerHandle,
    rx: mpsc::Receiver<SessionEvent>,
    window_handle: WindowHandle,
}
impl Session {
    pub fn spawn(id: u32, session_manager_handle: SessionManagerHandle) -> Result<SessionHandle> {
        let session = Session::new(id, session_manager_handle);
        session.run()
    }
    fn new(id: u32, session_manager_handle: SessionManagerHandle) -> Self {
        let (tx, rx) = mpsc::channel(10);
        let handle = SessionHandle { tx };
        let window_handle = Window::spawn(handle.clone()).unwrap();
        Self {
            id,
            session_manager_handle,
            handle,
            rx,
            window_handle,
        }
    }
    fn run(mut self) -> Result<SessionHandle> {
        let handle_clone = self.handle.clone();

        let _task = tokio::spawn({
            async move {
                loop {
                    tokio::select! {
                        Some(event) = self.rx.recv() => {
                            use SessionEvent::*;
                            match event {
                                UserInput(bytes) => {
                                    trace!("Session: UserInput");
                                    self.handle_user_input(bytes).await.unwrap();
                                },
                                WindowOutput(bytes) => {
                                    trace!("Session: WindowOutput");
                                    self.handle_window_output(bytes).await.unwrap();
                                },
                                NewConnection => {
                                    trace!("Session: WindowOutput");
                                    self.handle_new_connection().await.unwrap();
                                }
                                Kill => {
                                    trace!("Session: Kill");
                                    self.window_handle.kill().await.unwrap();
                                    break;
                                },
                            }
                        }
                    }
                }
            }
        });

        Ok(handle_clone)
    }

    async fn handle_user_input(&self, bytes: Bytes) -> Result<()> {
        self.window_handle.send_user_input(bytes).await;
        Ok(())
    }

    async fn handle_window_output(&self, bytes: Bytes) -> Result<()> {
        self.session_manager_handle
            .session_send_output(self.id, bytes)
            .await;
        Ok(())
    }

    async fn handle_new_connection(&self) -> Result<()> {
        self.window_handle.redraw().await;
        Ok(())
    }
}
#[derive(Debug, Clone)]
pub struct SessionHandle {
    tx: mpsc::Sender<SessionEvent>,
}
impl SessionHandle {
    pub async fn send_user_input(&mut self, bytes: Bytes) {
        self.tx.send(SessionEvent::UserInput(bytes)).await.unwrap();
    }
    pub async fn send_window_output(&mut self, bytes: Bytes) {
        self.tx
            .send(SessionEvent::WindowOutput(bytes))
            .await
            .unwrap();
    }
    pub async fn send_new_connection(&mut self) {
        self.tx.send(SessionEvent::NewConnection).await.unwrap();
    }
    pub async fn kill(&self) -> Result<()> {
        self.tx.send(SessionEvent::Kill).await.unwrap();
        Ok(())
    }

    fn is_alive(&self) -> bool {
        !self.tx.is_closed()
    }
}

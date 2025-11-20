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

#[allow(unused)]
#[derive(Debug)]
pub enum SessionEvent {
    // user input
    UserInput(Bytes),
    // user commands 
    //  - client id not needed anymore because session controls active window and
    //    window controls active pane which should be sufficient)
    UserConnection,
    UserSplitPane, 
    UserKillPane,

    // output
    WindowOutput(Bytes),
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
                    if let Some(event) = self.rx.recv().await {
                        use SessionEvent::*;
                        match event {
                            UserInput(bytes) => {
                                trace!("Session: UserInput");
                                self.handle_user_input(bytes).await.unwrap();
                            }
                            UserConnection => {
                                trace!("Session: UserConnection");
                                self.handle_new_connection().await.unwrap();
                            }
                            UserSplitPane => {
                                trace!("Session: UserSplitPane");
                                todo!()
                            }
                            UserKillPane => {
                                trace!("Session: UserKillPane");
                                todo!()
                            }
                            WindowOutput(bytes) => {
                                trace!("Session: WindowOutput");
                                self.handle_window_output(bytes).await.unwrap();
                            }
                            Kill => {
                                trace!("Session: Kill");
                                self.window_handle.kill().await.unwrap();
                                break;
                            }
                        }
                    }
                }
            }
        });

        Ok(handle_clone)
    }

    async fn handle_user_input(&self, bytes: Bytes) -> Result<()> {
        self.window_handle.send_user_input(bytes).await
    }

    async fn handle_window_output(&self, bytes: Bytes) -> Result<()> {
        self.session_manager_handle
            .session_send_output(self.id, bytes)
            .await
    }

    async fn handle_new_connection(&self) -> Result<()> {
        self.window_handle.redraw().await
    }
}
#[derive(Debug, Clone)]
pub struct SessionHandle {
    tx: mpsc::Sender<SessionEvent>,
}
#[allow(unused)]
impl SessionHandle {
    pub async fn send_user_input(&mut self, bytes: Bytes) -> Result<()> {
        Ok(self.tx.send(SessionEvent::UserInput(bytes)).await?)
    }
    pub async fn send_user_split_pane(&mut self) -> Result<()> {
        Ok(self.tx.send(SessionEvent::UserSplitPane).await?)
    }
    pub async fn send_user_kill_pane(&mut self) -> Result<()> {
        Ok(self.tx.send(SessionEvent::UserKillPane).await?)
    }
    pub async fn send_window_output(&mut self, bytes: Bytes) -> Result<()> {
        Ok(self.tx.send(SessionEvent::WindowOutput(bytes)).await?)
    }
    pub async fn send_new_connection(&mut self) -> Result<()> {
        Ok(self.tx.send(SessionEvent::UserConnection).await?)
    }
    pub async fn kill(&self) -> Result<()> {
        Ok(self.tx.send(SessionEvent::Kill).await?)
    }
}

use bytes::Bytes;
use tokio::sync::mpsc;
use tracing::Instrument;

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
    UserInput { bytes: Bytes },
    // user commands
    //  - client id not needed anymore because session controls active window and
    //    window controls active pane which should be sufficient)
    UserConnection,
    UserSplitPane,
    UserKillPane,
    Redraw,

    // output
    WindowOutput { bytes: Bytes },
    Kill,
}
use SessionEvent::*;

pub struct Session {
    id: u32,
    handle: SessionHandle,
    session_manager_handle: SessionManagerHandle,
    rx: mpsc::Receiver<SessionEvent>,
    window_handle: WindowHandle,
}
impl Session {
    #[instrument(skip(session_manager_handle), fields(session_id = id))]
    pub fn spawn(id: u32, session_manager_handle: SessionManagerHandle) -> Result<SessionHandle> {
        let session = Session::new(id, session_manager_handle);
        session.run()
    }
    #[instrument(skip(session_manager_handle), fields(session_id = id))]
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
    #[instrument(skip(self), fields(session_id = self.id))]
    fn run(mut self) -> Result<SessionHandle> {
        let span = tracing::Span::current();
        let handle_clone = self.handle.clone();

        let _task = tokio::spawn({
            async move {
                loop {
                    if let Some(event) = self.rx.recv().await {
                        match event {
                            UserInput { bytes } => {
                                trace!("Session: UserInput");
                                self.handle_user_input(bytes).await.unwrap();
                            }
                            UserConnection => {
                                debug!("Session: UserConnection");
                                self.handle_new_connection().await.unwrap();
                            }
                            UserSplitPane => {
                                debug!("Session: UserSplitPane");
                                todo!()
                            }
                            UserKillPane => {
                                debug!("Session: UserKillPane");
                                todo!()
                            }
                            WindowOutput { bytes } => {
                                trace!("Session: WindowOutput");
                                self.handle_window_output(bytes).await.unwrap();
                            }
                            Redraw => {
                                self.window_handle.redraw().await.unwrap();
                            }
                            Kill => {
                                debug!("Session: Kill");
                                self.window_handle.kill().await.unwrap();
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
    handle_method!(send_user_input, UserInput, bytes: Bytes);
    handle_method!(send_window_output, WindowOutput, bytes: Bytes);
    handle_method!(send_user_split_pane, UserSplitPane);
    handle_method!(send_user_kill_pane, UserKillPane);
    handle_method!(send_new_connection, UserConnection);
    handle_method!(redraw, Redraw);
    handle_method!(kill, Kill);
}

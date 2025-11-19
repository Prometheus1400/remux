use bytes::Bytes;
use tokio::sync::mpsc;
use tracing::trace;

use crate::{
    actor::{Actor, ActorHandle}, error::Result, session_manager::{SessionManager, SessionManagerHandle}, window::{Window, WindowHandle}
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
    pub fn new(id: u32, session_manager_handle: SessionManagerHandle) -> Self {
        let (tx, rx) = mpsc::channel(10);
        let handle = SessionHandle { tx };
        let window_handle = Window::new(handle.clone()).run().unwrap();
        Self {
            id,
            session_manager_handle,
            handle,
            rx,
            window_handle,
        }
    }

    async fn handle_user_input(&self, bytes: Bytes) -> Result<()> {
        self.window_handle.send_user_input(bytes).await;
        Ok(())
    }

    async fn handle_window_output(&self, bytes: Bytes) -> Result<()> {
        self.session_manager_handle.session_send_output(self.id, bytes).await;
        Ok(())
    }

    async fn handle_new_connection(&self) -> Result<()> {
        self.window_handle.redraw().await;
        Ok(())
    }
}
impl Actor<SessionHandle> for Session {
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
        self.tx.send(SessionEvent::WindowOutput(bytes)).await.unwrap();
    }
    pub async fn send_new_connection(&mut self) {
        self.tx.send(SessionEvent::NewConnection).await.unwrap();
    }
}
impl ActorHandle for SessionHandle {
    async fn kill(&self) -> Result<()> {
        self.tx.send(SessionEvent::Kill).await.unwrap();
        Ok(())
    }

    fn is_alive(&self) -> bool {
        !self.tx.is_closed()
    }
}

// impl Session {
//     pub fn new() -> Result<Self> {
//         Ok(Self {
//             pane: PaneBuilder::new().build()?,
//         })
//     }
//
//     pub async fn full_redraw(&mut self) {
//         self.pane.redraw().await;
//     }
//
//     pub async fn attach_stream(&mut self, stream: Arc<RwLock<UnixStream>>) -> DaemonTask {
//         // when this goes out of scope the subscriber should be dropped
//         let mut rx = self.pane.subscribe();
//         self.full_redraw().await;
//         let pane_tx = self.pane.get_sender().clone();
//         let mut closed_rx = self.pane.get_closed_watcher().clone();
//         let stream_task: DaemonTask = tokio::spawn(async move {
//             let mut buf = [0u8; 1024];
//             loop {
//                 tokio::select! {
//                     Ok(n) = async {
//                         let mut guard = stream.write().await;
//                         guard.read(&mut buf).await
//                     } => {
//                         if n > 0 {
//                             if pane_tx.send(Bytes::copy_from_slice(&buf[..n])).is_err() {
//                                 debug!("pane_tx: detected pane has terminated!");
//                                 // TODO: would need some logic to remove the pane from the session
//                                 break;
//                             }
//                         } else {
//                             debug!("Tcp client disconnected");
//                             break;
//                         }
//                     },
//                     Ok(bytes) = rx.recv() => {
//                         stream.write().await.write(&bytes).await.map_err(|_| Error::Custom("error sending to stream".to_owned()))?;
//                     },
//                     Ok(_) = closed_rx.changed() => {
//                         debug!("watch: detected pane terminated!");
//                         break;
//                     }
//                 }
//             }
//             Ok(())
//         });
//         stream_task
//     }
// }

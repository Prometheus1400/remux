use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
    vec,
};

use bytes::Bytes;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    sync::{Mutex, RwLock},
};
use tracing::debug;

use crate::{
    error::{Error, Result},
    pane::{Pane, PaneBuilder},
    types::NoResTask,
};

pub type SharedSessionTable = LazyLock<Arc<Mutex<SessionTable>>>;
pub static SHARED_SESSION_TABLE: SharedSessionTable =
    LazyLock::new(|| Arc::new(Mutex::new(SessionTable::new())));

pub struct SessionTable {
    sessions: HashMap<u16, Arc<Mutex<Session>>>,
}
impl SessionTable {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }

    pub fn get_or_create_session(&mut self, session_id: u16) -> Result<Arc<Mutex<Session>>> {
        if let None = self.sessions.get(&session_id) {
            self.sessions.insert(session_id, Arc::new(Mutex::new(Session::new()?)));
        }

        Ok(self.sessions.get(&session_id).unwrap().clone())
    }

    // pub fn get_active_session(&mut self) -> Option<&mut Session> {
    //     self.active_session.as_mut()
    // }
    //
    // pub fn new_active_session(&mut self, session: Session) {
    //     if let Some(prev_session) = self.active_session.take() {
    //         self.inactive_sessions.push(prev_session);
    //     }
    //     self.active_session = Some(session);
    // }
    //
    // pub fn attach_client(&mut self, stream: UnixStream) -> Result<()> {
    //     self.active_session
    //         .as_mut()
    //         .ok_or(Error::Custom(
    //             "trying to attach client when no active session".to_owned(),
    //         ))?
    //         .attach_stream(stream);
    //     Ok(())
    // }
}

// Session sould own the client connection when it is active
pub struct Session {
    pane: Pane,
}
impl Session {
    pub fn new() -> Result<Self> {
        Ok(Self {
            pane: PaneBuilder::new().build()?,
        })
    }

    pub fn attach_stream(&mut self, stream: Arc<RwLock<UnixStream>>) -> NoResTask {
        // when this goes out of scope the subscriber should be dropped
        let mut rx = self.pane.subscribe();
        let pane_tx = self.pane.get_sender().clone();
        let mut closed_rx = self.pane.get_closed_watcher().clone();
        let stream_task: NoResTask = tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            loop {
                tokio::select! {
                    Ok(n) = async {
                        let mut guard = stream.write().await;
                        guard.read(&mut buf).await
                    } => {
                        if n > 0 {
                            if pane_tx.send(Bytes::copy_from_slice(&buf[..n])).is_err() {
                                debug!("pane_tx: detected pane has terminated!");
                                // TODO: would need some logic to remove the pane from the session
                                break;
                            }
                        } else {
                            debug!("Tcp client disconnected");
                            break;
                        }
                    },
                    Ok(bytes) = rx.recv() => {
                        stream.write().await.write(&bytes).await.map_err(|_| Error::Custom("error sending to stream".to_owned()))?;
                    },
                    _ = closed_rx.changed() => {
                        if *closed_rx.borrow() {
                            debug!("watch: detected pane terminated!");
                            break;
                        }
                    }
                }
            }
            Ok(())
        });
        stream_task
    }
}

use std::{
    sync::{Arc, LazyLock},
    vec,
};

use bytes::Bytes;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    sync::Mutex,
    task::JoinHandle,
};
use tracing::{info, instrument};

use crate::{
    error::{Error, Result},
    pane::{Focused, Hidden, Pane, PaneBuilder, PaneState},
};
type Task = JoinHandle<std::result::Result<(), Error>>;
pub type SharedSessionTable = LazyLock<Arc<Mutex<SessionTable>>>;
pub static SHARED_SESSION_TABLE: LazyLock<Arc<SessionTable>> =
    LazyLock::new(|| Arc::new(SessionTable::new()));

pub struct SessionTable {
    active_session: Option<Session<Active, Focused>>,
    inactive_sessions: Vec<Session<Inactive, Hidden>>,
}
impl SessionTable {
    pub fn new() -> Self {
        Self {
            active_session: None,
            inactive_sessions: vec![],
        }
    }

    pub fn get_active_session(&mut self) -> Option<&mut Session<Active, Focused>> {
        self.active_session.as_mut()
    }

    pub fn new_active_session(&mut self, session: Session<Active, Focused>) {
        if let Some(prev_session) = self.active_session.take() {
            let deactivated_session = prev_session.hide();
            self.inactive_sessions.push(deactivated_session);
        }

        self.active_session = Some(session);
    }

    pub fn attach_client(&mut self, stream: UnixStream) -> Result<()> {
        self.active_session
            .as_mut()
            .ok_or(Error::Custom(
                "trying to attach client when no active session".to_owned(),
            ))?
            .attach_client(stream);
        Ok(())
    }
}

pub trait SessionState {}
pub struct Active {}
impl SessionState for Active {}
pub struct Inactive {}
impl SessionState for Inactive {}

struct ClientAttachment {
    pub stream: UnixStream,
    pub task: Task,
}

// Session sould own the client connection when it is active
pub struct Session<State, P>
where
    State: SessionState,
    P: PaneState,
{
    pane: Pane<P>,
    stream: Option<UnixStream>,
    client_task: Option<Task>,
    _state: std::marker::PhantomData<State>,
}
impl Session<Active, Focused> {
    pub fn new() -> Result<Self> {
        Ok(Self {
            pane: PaneBuilder::new().build()?,
            // stream: Some(stream),
            stream: None,
            client_task: None,
            _state: std::marker::PhantomData,
        })
    }

    pub fn hide(self) -> Session<Inactive, Hidden> {
        if let Some(task) = self.client_task {
            task.abort();
        }
        Session::<Inactive, Hidden> {
            pane: self.pane.hide(),
            stream: None,
            client_task: None,
            _state: std::marker::PhantomData,
        }
    }

    pub async fn attach_client(&mut self, mut stream: UnixStream) -> Result<()> {
        // when this goes out of scope the subscriber should be dropped
        let mut rx = self.pane.subscribe();
        let pane_tx = self.pane.get_sender().clone();
        let client_task: Task = tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            loop {
                tokio::select! {
                    Ok(n) = stream.read(&mut buf) => {
                        if n > 0 {
                            if pane_tx.send(Bytes::copy_from_slice(&buf[..n])).is_err() {
                                info!("pane has terminated!");
                                // TODO: would need some logic to remove the pane from the session
                                break;
                            }
                        } else {
                            info!("Tcp client disconnected");
                            break;
                        }
                    },
                    Ok(bytes) = rx.recv() => {
                        stream.write(&bytes).await.map_err(|_| Error::Custom("error sending to stream".to_owned()))?;
                    }
                }
            }
            Ok(())
        });
        let _ = tokio::try_join!(client_task)?;
        Ok(())
    }
}
impl Session<Inactive, Hidden> {
    pub fn focus(self) -> Session<Active, Focused> {
        Session::<Active, Focused> {
            pane: self.pane.focus(),
            stream: None,
            client_task: None,
            _state: std::marker::PhantomData,
        }
    }
}

use std::sync::Arc;

use tokio::{
    net::UnixStream,
    sync::{Mutex, RwLock},
};

use crate::{session::Session, types::NoResTask};

pub struct ClientSession {
    stream: Arc<RwLock<UnixStream>>,
    attached_session: Option<Arc<Mutex<Session>>>,
    session_task: Option<NoResTask>,
}
impl ClientSession {
    pub fn new(stream: UnixStream) -> Self {
        Self {
            stream: Arc::new(RwLock::new(stream)),
            attached_session: None,
            session_task: None,
        }
    }

    pub async fn block(&mut self) {
        if let Some(task) = self.session_task.take() {
            let _ = tokio::join!(task);
        }
    }

    pub async fn attach_to_session(&mut self, session: Arc<Mutex<Session>>) {
        self.detach();
        let mut guard = session.lock().await;
        let session_task = guard.attach_stream(self.stream.clone());
        self.session_task = Some(session_task);
    }

    pub fn detach(&mut self) {
        if let Some(task) = &self.session_task {
            task.abort();
        }
        self.session_task = None;
        self.attached_session = None;
    }
}

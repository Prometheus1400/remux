use std::fs::{File, remove_file};

use remux_core::{
    daemon_utils::{get_sock_path, lock_daemon_file},
    messages::{self, RequestBody},
};
use tokio::net::{UnixListener, UnixStream};

use crate::{
    actors::{
        client::Client,
        session_manager::{SessionManager, SessionManagerHandle},
    },
    error::Result,
    prelude::*,
};

pub struct RemuxDaemon {
    _daemon_file: File, // daemon must hold the exclusive file lock while it is alive and running
    session_manager_handle: SessionManagerHandle,
}

impl RemuxDaemon {
    /// Makes sure there can only ever be once instance at the
    /// process level through use of OS level file locks
    pub fn new() -> Result<Self> {
        let session_manager_handle = SessionManager::spawn().unwrap();
        Ok(Self {
            _daemon_file: lock_daemon_file()?,
            session_manager_handle,
        })
    }

    #[instrument(skip(self))]
    pub async fn listen(&self) -> Result<()> {
        let socket_path = get_sock_path()?;

        if socket_path.exists() {
            remove_file(&socket_path)?;
        }

        info!("unix socket path: {:?}", socket_path);
        let listener = UnixListener::bind(socket_path)?;
        loop {
            let (stream, addr) = listener.accept().await?;
            info!("accepting connection from: {:?}", addr.as_pathname());
            if let Err(e) = handle_communication(self.session_manager_handle.clone(), stream).await
            {
                error!("{e}");
            }
        }
    }
}

#[instrument(skip(stream))]
async fn handle_communication(
    session_manager_handle: SessionManagerHandle,
    mut stream: UnixStream,
) -> Result<()> {
    let req = messages::read_req(&mut stream).await?;
    match req.body {
        RequestBody::Attach { session_id } => {
            debug!("running new client actor");
            let mut client = Client::spawn(stream, session_manager_handle).unwrap();
            client.attach_to_session(session_id).await;
        }
        RequestBody::SessionsList => {
            todo!()
            // info!("handling sesions list");
            // let sessions = SHARED_SESSION_TABLE.lock().await.get_sessions();
            // messages::write_message(
            //     &mut stream,
            //     &ResponseMessage::new(req.get_id(), ResponseBody::SessionsList { sessions }),
            // )
            // .await?;
        }
    };
    Ok(())
}

#[cfg(test)]
mod test {
    #[tokio::test]
    async fn name() {}
}

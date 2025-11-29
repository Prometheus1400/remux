use std::fs::{File, remove_file};

use remux_core::{
    comm,
    daemon_utils::{get_sock_path, lock_daemon_file},
    messages::RequestBody,
};
use tokio::net::{UnixListener, UnixStream};

use crate::{
    actors::{
        client_connection::ClientConnection,
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
            let (stream, _) = listener.accept().await?;
            info!("accepting connection");
            if let Err(e) = handle_comm(self.session_manager_handle.clone(), stream).await {
                error!("{e}");
            }
        }
    }
}

#[instrument(skip(session_manager_handle, stream))]
async fn handle_comm(session_manager_handle: SessionManagerHandle, mut stream: UnixStream) -> Result<()> {
    let req = comm::read_req(&mut stream).await?;
    match req.body {
        RequestBody::Attach { session_id } => {
            debug!("running new client actor");
            let client = ClientConnection::spawn(stream, session_manager_handle).unwrap();
            client.attach_to_session(session_id).await.unwrap();
        }
        RequestBody::SessionsList => {
            todo!()
        }
    };
    Ok(())
}

#[cfg(test)]
mod test {
    #[tokio::test]
    async fn name() {}
}

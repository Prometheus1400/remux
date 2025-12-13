use std::fs::{File, remove_file};

use remux_core::{
    comm,
    daemon_utils::{get_sock_path, lock_daemon_file},
};
use tokio::net::{UnixListener, UnixStream};

use crate::{
    actors::{
        client_connection::ClientConnection,
        session_manager::{SessionManager, SessionManagerHandle},
    },
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
        let session_manager_handle = SessionManager::spawn()?;
        Ok(Self {
            _daemon_file: lock_daemon_file()?,
            session_manager_handle,
        })
    }

    #[instrument(skip(self), name = "Daemon")]
    pub async fn listen(&self) -> Result<()> {
        let socket_path = get_sock_path()?;

        if socket_path.exists() {
            remove_file(&socket_path)?;
        }

        info!(path = ?socket_path, "Connecting to unix socket");
        let listener = UnixListener::bind(socket_path)?;
        loop {
            let (stream, _) = listener.accept().await?;
            info!("Accepting connection");
            if let Err(e) = handle_message(self.session_manager_handle.clone(), stream).await {
                error!("{e}");
            }
        }
    }
}

#[instrument(skip(session_manager_handle, stream))]
async fn handle_message(session_manager_handle: SessionManagerHandle, mut stream: UnixStream) -> Result<()> {
    use remux_core::messages::request::{self, DaemonRequestMessage, DaemonRequestMessageBody};

    let req: DaemonRequestMessage = comm::read_message(&mut stream).await?;
    info!(request=?req, "Handling request");
    match req.body {
        DaemonRequestMessageBody::Attach(request::Attach { id, session_name, create }) => {
            info!(
                connecting_session = session_name,
                create = create,
                "Creating new client actor"
            );
            let _client = ClientConnection::spawn(id, stream, session_manager_handle, &session_name)?;
        }
    };
    Ok(())
}

#[cfg(test)]
mod test {
    #[tokio::test]
    async fn name() {}
}

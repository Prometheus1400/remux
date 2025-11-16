use std::fs::{File, remove_file};

use remux_core::{
    daemon_utils::{get_sock_path, lock_daemon_file},
    messages::{self, Message, RequestBody, ResponseBody, ResponseMessage},
};
use tokio::net::{UnixListener, UnixStream};
use tracing::{error, info, instrument};

use crate::{client_session::ClientSession, error::Result, session::SHARED_SESSION_TABLE};

pub struct RemuxDaemon {
    _daemon_file: File, // daemon must hold the exclusive file lock while it is alive and running
}

impl RemuxDaemon {
    /// Makes sure there can only ever be once instance at the
    /// process level through use of OS level file locks
    pub fn new() -> Result<Self> {
        Ok(Self {
            _daemon_file: lock_daemon_file()?,
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
            tokio::spawn(async move {
                if let Err(e) = handle_communication(stream).await {
                    error!("{e}");
                }
            });
        }
    }
}

// TODO: this is a hack just for testing
// #[instrument]
async fn attach_client(mut client_session: ClientSession, session_id: u16) -> Result<()> {
    let session = SHARED_SESSION_TABLE
        .lock()
        .await
        .get_or_create_session(session_id)?;
    client_session.attach_to_session(session).await;
    client_session.block().await;
    Ok(())
}

#[instrument(skip(stream))]
async fn handle_communication(mut stream: UnixStream) -> Result<()> {
    let req = messages::read_req(&mut stream).await?;
    match req.body {
        RequestBody::Attach { session_id } => {
            let client_session = ClientSession::new(stream);
            info!("attaching client");
            attach_client(client_session, session_id).await?;
        }
        RequestBody::SessionsList => {
            info!("handling sesions list");
            let sessions = SHARED_SESSION_TABLE.lock().await.get_sessions();
            messages::write_message(
                &mut stream,
                &ResponseMessage::new(req.get_id(), ResponseBody::SessionsList { sessions }),
            )
            .await?;
        }
    };
    Ok(())
}

#[cfg(test)]
mod test {
    #[tokio::test]
    async fn name() {}
}

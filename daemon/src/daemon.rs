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
        DaemonRequestMessageBody::Attach(request::Attach {
            id,
            session_name,
            create,
            rows,
            cols,
        }) => {
            info!(
                connecting_session = session_name,
                create = create,
                "Creating new client actor"
            );
            let _client = ClientConnection::spawn(id, stream, session_manager_handle, &session_name, rows, cols)?;
        }
    };
    Ok(())
}

#[cfg(test)]
mod test {
    #![allow(clippy::unwrap_used)]

    use bytes::Bytes;
    use remux_core::{
        comm,
        events::{CliEvent, DaemonEvent},
        messages::{
            RequestBuilder, ResponseMessage, ResponseResult,
            request,
            response,
        },
    };
    use tokio::time::{Duration, timeout};
    use tokio::io::AsyncWriteExt;
    use tokio::net::UnixStream;
    use uuid::Uuid;

    use super::handle_message;
    use crate::actors::session_manager::SessionManager;
    use crate::prelude::Result;

    async fn attach_client(
        session_manager_handle: crate::actors::session_manager::SessionManagerHandle,
        session_name: &str,
    ) -> (tokio::task::JoinHandle<Result<()>>, UnixStream) {
        let (mut client_stream, daemon_stream) = UnixStream::pair().unwrap();
        let request = RequestBuilder::default()
            .body(request::Attach {
                id: Uuid::new_v4(),
                session_name: session_name.to_owned(),
                create: true,
                rows: 20,
                cols: 60,
            })
            .build();

        comm::send_message(&mut client_stream, &request).await.unwrap();

        let task = tokio::spawn(handle_message(session_manager_handle, daemon_stream));
        (task, client_stream)
    }

    async fn recv_until_disconnected(stream: &mut UnixStream) -> Result<()> {
        timeout(Duration::from_secs(2), async {
            loop {
                match comm::recv_daemon_event(stream).await? {
                    DaemonEvent::Disconnected => return Ok(()),
                    DaemonEvent::Raw(_) => {}
                }
            }
        })
        .await
        .map_err(|err| color_eyre::eyre::eyre!(err))?
    }

    async fn recv_daemon_event_with_timeout(stream: &mut UnixStream, duration: Duration) -> Result<Option<DaemonEvent>> {
        match timeout(duration, comm::recv_daemon_event(stream)).await {
            Ok(result) => result.map(Some).map_err(Into::into),
            Err(_) => Ok(None),
        }
    }

    #[tokio::test]
    async fn handle_message_completes_attach_handshake() -> Result<()> {
        let session_manager_handle = SessionManager::spawn()?;
        let (task, mut client_stream) = attach_client(session_manager_handle, "alpha").await;

        let response: ResponseMessage<response::Attach> = comm::read_message(&mut client_stream).await?;

        match response.result {
            ResponseResult::Success(body) => assert!(body.attached),
            other => panic!("expected successful attach response, got {other:?}"),
        }
        comm::send_event(&mut client_stream, CliEvent::Detach).await?;
        recv_until_disconnected(&mut client_stream).await?;
        task.await.unwrap()?;
        Ok(())
    }

    #[tokio::test]
    async fn handle_message_routes_detach_to_disconnected_event() -> Result<()> {
        let session_manager_handle = SessionManager::spawn()?;
        let (task, mut client_stream) = attach_client(session_manager_handle, "beta").await;

        let _: ResponseMessage<response::Attach> = comm::read_message(&mut client_stream).await?;
        comm::send_event(&mut client_stream, CliEvent::Detach).await?;
        recv_until_disconnected(&mut client_stream).await?;
        task.await.unwrap()?;
        Ok(())
    }

    #[tokio::test]
    async fn handle_message_sends_first_render_without_waiting_for_resize_event() -> Result<()> {
        let session_manager_handle = SessionManager::spawn()?;
        let (task, mut client_stream) = attach_client(session_manager_handle, "pre-sized").await;

        let _: ResponseMessage<response::Attach> = comm::read_message(&mut client_stream).await?;
        let event = recv_daemon_event_with_timeout(&mut client_stream, Duration::from_secs(2)).await?;
        assert!(matches!(event, Some(DaemonEvent::Raw(bytes)) if !bytes.is_empty()));

        comm::send_event(&mut client_stream, CliEvent::Detach).await?;
        recv_until_disconnected(&mut client_stream).await?;
        task.await.unwrap()?;
        Ok(())
    }

    #[tokio::test]
    async fn handle_message_accepts_resize_and_raw_input_without_protocol_failure() -> Result<()> {
        let session_manager_handle = SessionManager::spawn()?;
        let (task, mut client_stream) = attach_client(session_manager_handle, "gamma").await;

        let _: ResponseMessage<response::Attach> = comm::read_message(&mut client_stream).await?;
        comm::send_event(&mut client_stream, CliEvent::TerminalResize { rows: 20, cols: 60 }).await?;
        comm::send_event(&mut client_stream, CliEvent::Raw(Bytes::from_static(b"echo test\r"))).await?;
        comm::send_event(&mut client_stream, CliEvent::Detach).await?;

        recv_until_disconnected(&mut client_stream).await?;
        task.await.unwrap()?;
        Ok(())
    }

    #[tokio::test]
    async fn handle_message_rejects_invalid_request_payload() -> Result<()> {
        let session_manager_handle = SessionManager::spawn()?;
        let (mut client_stream, daemon_stream) = UnixStream::pair()?;
        let task = tokio::spawn(handle_message(session_manager_handle, daemon_stream));

        client_stream.write_all(&4u32.to_be_bytes()).await?;
        client_stream.write_all(b"nope").await?;

        let err = task.await.unwrap().unwrap_err();
        assert!(err.to_string().contains("Serialization error"));
        Ok(())
    }
}

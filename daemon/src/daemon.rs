use std::{
    fs::{File, remove_file},
    sync::Arc,
};

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
    lua::config::ConfigRuntime,
    prelude::*,
};

pub struct RemuxDaemon {
    _daemon_file: File, // daemon must hold the exclusive file lock while it is alive and running
    config_runtime: Arc<ConfigRuntime>,
    session_manager_handle: SessionManagerHandle,
}

impl RemuxDaemon {
    /// Makes sure there can only ever be once instance at the
    /// process level through use of OS level file locks
    pub fn new() -> Result<Self> {
        let config_runtime = Arc::new(ConfigRuntime::load()?);
        let session_manager_handle = SessionManager::spawn(config_runtime.clone())?;
        Ok(Self {
            _daemon_file: lock_daemon_file()?,
            config_runtime,
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
            if let Err(e) =
                handle_message(self.session_manager_handle.clone(), self.config_runtime.clone(), stream).await
            {
                error!("{e}");
            }
        }
    }
}

#[instrument(skip(session_manager_handle, config_runtime, stream))]
async fn handle_message(
    session_manager_handle: SessionManagerHandle,
    config_runtime: Arc<ConfigRuntime>,
    mut stream: UnixStream,
) -> Result<()> {
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
            let _client = ClientConnection::spawn(
                req.id,
                id,
                stream,
                session_manager_handle,
                config_runtime,
                &session_name,
                rows,
                cols,
            )?;
        }
    };
    Ok(())
}

#[cfg(test)]
mod test {
    #![allow(clippy::unwrap_used)]

    use std::sync::Arc;

    use bytes::Bytes;
    use remux_core::{
        comm,
        events::{CliEvent, DaemonEvent},
        messages::{RequestBuilder, ResponseMessage, ResponseResult, request, response},
    };
    use serial_test::serial;
    use tokio::{
        io::AsyncWriteExt,
        net::UnixStream,
        time::{Duration, timeout},
    };
    use uuid::Uuid;

    use super::handle_message;
    use crate::{
        actors::session_manager::{SessionManager, SessionManagerHandle},
        lua::config::ConfigRuntime,
        prelude::Result,
    };

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

        comm::send_request(&mut client_stream, &request).await.unwrap();

        let task = tokio::spawn(handle_message(
            session_manager_handle,
            Arc::new(ConfigRuntime::load().unwrap()),
            daemon_stream,
        ));
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

    async fn recv_daemon_event_with_timeout(
        stream: &mut UnixStream,
        duration: Duration,
    ) -> Result<Option<DaemonEvent>> {
        match timeout(duration, comm::recv_daemon_event(stream)).await {
            Ok(result) => result.map(Some).map_err(Into::into),
            Err(_) => Ok(None),
        }
    }

    async fn shutdown_session_manager(session_manager_handle: SessionManagerHandle) -> Result<()> {
        session_manager_handle.kill().await?;
        tokio::time::sleep(Duration::from_millis(50)).await;
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn handle_message_completes_attach_handshake() -> Result<()> {
        let session_manager_handle = SessionManager::spawn(Arc::new(ConfigRuntime::load()?))?;
        let (task, mut client_stream) = attach_client(session_manager_handle.clone(), "alpha").await;

        let response: ResponseMessage<response::Attach> = comm::read_message(&mut client_stream).await?;

        match response.result {
            ResponseResult::Success(body) => assert!(body.attached),
            other => panic!("expected successful attach response, got {other:?}"),
        }
        comm::send_event(&mut client_stream, CliEvent::Detach).await?;
        recv_until_disconnected(&mut client_stream).await?;
        task.await.unwrap()?;
        shutdown_session_manager(session_manager_handle).await?;
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn handle_message_routes_detach_to_disconnected_event() -> Result<()> {
        let session_manager_handle = SessionManager::spawn(Arc::new(ConfigRuntime::load()?))?;
        let (task, mut client_stream) = attach_client(session_manager_handle.clone(), "beta").await;

        let _: ResponseMessage<response::Attach> = comm::read_message(&mut client_stream).await?;
        comm::send_event(&mut client_stream, CliEvent::Detach).await?;
        recv_until_disconnected(&mut client_stream).await?;
        task.await.unwrap()?;
        shutdown_session_manager(session_manager_handle).await?;
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn handle_message_parses_raw_detach_binding_server_side() -> Result<()> {
        let session_manager_handle = SessionManager::spawn(Arc::new(ConfigRuntime::load()?))?;
        let (task, mut client_stream) = attach_client(session_manager_handle.clone(), "bound-detach").await;

        let _: ResponseMessage<response::Attach> = comm::read_message(&mut client_stream).await?;
        comm::send_event(&mut client_stream, CliEvent::Raw(Bytes::from_static(b"\x02d"))).await?;

        recv_until_disconnected(&mut client_stream).await?;
        task.await.unwrap()?;
        shutdown_session_manager(session_manager_handle).await?;
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn handle_message_sends_first_render_without_waiting_for_resize_event() -> Result<()> {
        let session_manager_handle = SessionManager::spawn(Arc::new(ConfigRuntime::load()?))?;
        let (task, mut client_stream) = attach_client(session_manager_handle.clone(), "pre-sized").await;

        let _: ResponseMessage<response::Attach> = comm::read_message(&mut client_stream).await?;
        let event = recv_daemon_event_with_timeout(&mut client_stream, Duration::from_secs(2)).await?;
        assert!(matches!(event, Some(DaemonEvent::Raw(bytes)) if !bytes.is_empty()));

        comm::send_event(&mut client_stream, CliEvent::Detach).await?;
        recv_until_disconnected(&mut client_stream).await?;
        task.await.unwrap()?;
        shutdown_session_manager(session_manager_handle).await?;
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn handle_message_accepts_resize_without_protocol_failure() -> Result<()> {
        let session_manager_handle = SessionManager::spawn(Arc::new(ConfigRuntime::load()?))?;
        let (task, mut client_stream) = attach_client(session_manager_handle.clone(), "gamma").await;

        let _: ResponseMessage<response::Attach> = comm::read_message(&mut client_stream).await?;
        comm::send_event(&mut client_stream, CliEvent::TerminalResize { rows: 20, cols: 60 }).await?;
        comm::send_event(&mut client_stream, CliEvent::Detach).await?;

        recv_until_disconnected(&mut client_stream).await?;
        task.await.unwrap()?;
        shutdown_session_manager(session_manager_handle).await?;
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn handle_message_rejects_invalid_request_payload() -> Result<()> {
        let session_manager_handle = SessionManager::spawn(Arc::new(ConfigRuntime::load()?))?;
        let (mut client_stream, daemon_stream) = UnixStream::pair()?;
        let task = tokio::spawn(handle_message(
            session_manager_handle.clone(),
            Arc::new(ConfigRuntime::load()?),
            daemon_stream,
        ));

        client_stream.write_all(&4u32.to_be_bytes()).await?;
        client_stream.write_all(b"nope").await?;

        let err = task.await.unwrap().unwrap_err();
        assert!(err.to_string().contains("Serialization error"));
        shutdown_session_manager(session_manager_handle).await?;
        Ok(())
    }
}

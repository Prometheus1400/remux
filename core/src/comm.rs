use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};

use crate::{
    error::ResponseError,
    events::{CliEvent, DaemonEvent},
    messages::{CliRequestMessage, Message, RequestBody, ResponseMessage, ResponseResult},
    prelude::*,
};

pub async fn send_event<E: Serialize>(stream: &mut UnixStream, event: E) -> Result<()> {
    let bytes = serde_json::to_vec(&event)?;
    let num_bytes = bytes.len() as u32;
    let _written = stream.write(&num_bytes.to_be_bytes()).await?;
    let _written = stream.write(&bytes).await?;
    Ok(())
}

pub async fn recv_cli_event(stream: &mut UnixStream) -> Result<CliEvent> {
    recv_event(stream).await
}

pub async fn recv_daemon_event(stream: &mut UnixStream) -> Result<DaemonEvent> {
    recv_event(stream).await
}

async fn recv_event<E: DeserializeOwned>(stream: &mut UnixStream) -> Result<E> {
    let mut num_bytes = [0u8; 4];
    stream.read_exact(&mut num_bytes).await?;
    let num_bytes = u32::from_be_bytes(num_bytes);

    let mut message_bytes = vec![0u8; num_bytes as usize];
    stream.read_exact(&mut message_bytes).await?;

    Ok(serde_json::from_slice(&message_bytes)?)
}

pub async fn send_message(stream: &mut UnixStream, message: &impl Message) -> Result<()> {
    let bytes = serde_json::to_vec(message)?;
    let num_bytes = bytes.len() as u32;

    let _written = stream.write(&num_bytes.to_be_bytes()).await?;
    let _written = stream.write(&bytes).await?;
    Ok(())
}

pub async fn read_message<M: Message>(stream: &mut UnixStream) -> Result<M> {
    let mut num_bytes = [0u8; 4];
    stream.read_exact(&mut num_bytes).await?;
    let num_bytes = u32::from_be_bytes(num_bytes);
    let mut message_bytes = vec![0u8; num_bytes as usize];
    stream.read_exact(&mut message_bytes).await?;
    let res = serde_json::from_slice(&message_bytes)?;
    Ok(res)
}

pub async fn send_and_recv_message<B>(stream: &mut UnixStream, req: &CliRequestMessage<B>) -> Result<B::ResponseBody>
where
    B: RequestBody + Serialize + for<'de> Deserialize<'de>,
{
    // let req_id = req.id;
    send_message(stream, req).await?;
    let res: ResponseMessage<B::ResponseBody> = read_message(stream).await?;
    // let res_id = res.id;
    // if req_id != res_id {
    //     return Err(Error::Response(ResponseError::UnexpectedId { expected: req_id, actual: res_id }));
    // }
    match res.result {
        ResponseResult::Success(body) => Ok(body),
        ResponseResult::Failure { message } => Err(Error::Response(ResponseError::Status(message))),
    }
}

#[cfg(test)]
mod test {
    #![allow(clippy::unwrap_used)]
    use std::{fs::remove_file, path::PathBuf};

    use tokio::net::UnixListener;
    use uuid::Uuid;

    use super::*;
    use crate::{
        constants::TEMP_SOCK_DIR,
        events::{CliEvent, DaemonEvent},
        messages::{
            RequestBuilder, ResponseBuilder,
            request::{self, DaemonRequestMessage, DaemonRequestMessageBody},
            response,
        },
    };

    #[tokio::test]
    async fn test_tcp_message() -> Result<()> {
        // Bind server
        let temp_dir = PathBuf::from(TEMP_SOCK_DIR);
        if temp_dir.exists() {
            remove_file(&temp_dir)?;
        }

        let listener = UnixListener::bind(temp_dir)?;
        let addr = listener.local_addr()?;

        let attach = request::Attach {
            id: Uuid::new_v4(),
            session_name: "session".to_owned(),
            create: true,
            rows: 24,
            cols: 80,
        };
        let cli_req = RequestBuilder::default().body(attach.clone()).build();
        let daemon_req = DaemonRequestMessage {
            id: cli_req.id,
            body: DaemonRequestMessageBody::Attach(attach),
        };

        let attach_response = response::Attach {
            attached: true,
            initial_server_snapshot: None,
        };
        let res = ResponseBuilder::default()
            .result(ResponseResult::Success(attach_response.clone()))
            .build();

        // Spawn server
        let server: tokio::task::JoinHandle<Result<()>> = tokio::spawn({
            let res = res.clone();
            async move {
                let (mut socket, _) = listener.accept().await?;
                let msg1 = read_message::<DaemonRequestMessage>(&mut socket).await.unwrap();
                assert_eq!(msg1, daemon_req);
                send_message(&mut socket, &res).await.unwrap();
                Ok(())
            }
        });

        // Connect client
        let mut client = UnixStream::connect(addr.as_pathname().unwrap()).await.unwrap();
        let res1 = send_and_recv_message(&mut client, &cli_req).await.unwrap();
        assert_eq!(res1, attach_response);
        server.await.unwrap()?;
        Ok(())
    }

    #[test]
    fn attach_response_deserializes_old_and_new_shapes() {
        let old_json = r#"{"id":1,"result":{"type":"Success","initial_server_snapshot":{"sessions":[],"active_session":null}}}"#;
        let new_json = r#"{"id":1,"result":{"type":"Success","attached":true}}"#;

        let old: ResponseMessage<response::Attach> = serde_json::from_str(old_json).unwrap();
        let new: ResponseMessage<response::Attach> = serde_json::from_str(new_json).unwrap();

        assert!(matches!(old.result, ResponseResult::Success(_)));
        assert!(matches!(new.result, ResponseResult::Success(_)));
    }

    #[tokio::test]
    async fn cli_event_round_trip_over_stream() -> Result<()> {
        let (mut sender, mut receiver) = UnixStream::pair()?;

        let send = tokio::spawn(async move { send_event(&mut sender, CliEvent::TerminalResize { rows: 24, cols: 80 }).await });
        let event = recv_cli_event(&mut receiver).await?;

        send.await.unwrap()?;
        match event {
            CliEvent::TerminalResize { rows, cols } => {
                assert_eq!((rows, cols), (24, 80));
            }
            other => panic!("unexpected event: {other:?}"),
        }
        Ok(())
    }

    #[tokio::test]
    async fn daemon_event_round_trip_over_stream() -> Result<()> {
        let (mut sender, mut receiver) = UnixStream::pair()?;

        let send = tokio::spawn(async move { send_event(&mut sender, DaemonEvent::Disconnected).await });
        let event = recv_daemon_event(&mut receiver).await?;

        send.await.unwrap()?;
        assert!(matches!(event, DaemonEvent::Disconnected));
        Ok(())
    }

    #[tokio::test]
    async fn send_and_recv_message_returns_response_failure_as_error() -> Result<()> {
        let (mut client, mut server) = UnixStream::pair()?;
        let attach = request::Attach {
            id: Uuid::new_v4(),
            session_name: "session".to_owned(),
            create: true,
            rows: 24,
            cols: 80,
        };
        let cli_req = RequestBuilder::default().body(attach).build();
        let response = ResponseBuilder::default()
            .result(ResponseResult::<response::Attach>::Failure {
                message: "attach failed".to_owned(),
            })
            .build();

        let server_task = tokio::spawn(async move {
            let _: DaemonRequestMessage = read_message(&mut server).await.unwrap();
            send_message(&mut server, &response).await.unwrap();
        });

        let err = send_and_recv_message(&mut client, &cli_req).await.unwrap_err();
        server_task.await.unwrap();
        assert!(matches!(err, Error::Response(ResponseError::Status(message)) if message == "attach failed"));
        Ok(())
    }

    #[tokio::test]
    async fn read_message_fails_for_truncated_payload() -> Result<()> {
        let (mut writer, mut reader) = UnixStream::pair()?;

        let writer_task = tokio::spawn(async move {
            writer.write_all(&5u32.to_be_bytes()).await.unwrap();
            writer.write_all(b"{}").await.unwrap();
        });

        let err = read_message::<ResponseMessage<response::Attach>>(&mut reader).await.unwrap_err();
        writer_task.await.unwrap();
        assert!(matches!(err, Error::IO(_)));
        Ok(())
    }

    #[tokio::test]
    async fn recv_event_fails_for_invalid_json() -> Result<()> {
        let (mut writer, mut reader) = UnixStream::pair()?;

        let writer_task = tokio::spawn(async move {
            writer.write_all(&4u32.to_be_bytes()).await.unwrap();
            writer.write_all(b"nope").await.unwrap();
        });

        let err = recv_cli_event(&mut reader).await.unwrap_err();
        writer_task.await.unwrap();
        assert!(matches!(err, Error::SerializationError(_)));
        Ok(())
    }
}

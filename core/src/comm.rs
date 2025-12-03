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
    println!("reading message");
    let mut num_bytes = [0u8; 4];
    stream.read_exact(&mut num_bytes).await?;
    let num_bytes = u32::from_be_bytes(num_bytes);
    let mut message_bytes = vec![0u8; num_bytes as usize];
    stream.read_exact(&mut message_bytes).await?;
    println!("reading message2");
    let res = serde_json::from_slice(&message_bytes)?;
    println!("here");
    Ok(res)
}

pub async fn send_and_recv_message<B>(stream: &mut UnixStream, req: &CliRequestMessage<B>) -> Result<B::ResponseBody>
where
    B: RequestBody + Serialize + for<'de> Deserialize<'de>,
{
    let req_id = req.id;
    send_message(stream, req).await?;
    let res: ResponseMessage<B::ResponseBody> = read_message(stream).await?;
    let res_id = res.id;
    // if req_id != res_id {
    //     return Err(Error::Response(ResponseError::UnexpectedId { expected: req_id, actual: res_id }));
    // }
    match res.result {
        ResponseResult::Success(body) => Ok(body),
        ResponseResult::Failure(msg) => Err(Error::Response(ResponseError::Status(msg))),
    }
}

#[cfg(test)]
mod test {
    #![allow(clippy::unwrap_used)]
    use std::{fs::remove_file, path::PathBuf};

    use tokio::net::UnixListener;

    use super::*;
    use crate::{
        constants::TEMP_SOCK_DIR,
        messages::{
            RequestBuilder, ResponseBuilder,
            request::{self, DaemonRequestMessage, DaemonRequestMessageBody},
            response,
        },
        states::DaemonState,
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
            session_id: 1,
            create: true,
        };
        let cli_req = RequestBuilder::default().body(attach.clone()).build();
        let daemon_req = DaemonRequestMessage {
            id: cli_req.id,
            body: DaemonRequestMessageBody::Attach(attach),
        };

        let attach_response = response::Attach {
            initial_daemon_state: DaemonState::default(),
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
}

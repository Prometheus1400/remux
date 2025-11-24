use serde::{Serialize, de::DeserializeOwned};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};

use crate::{
    events::{CliEvent, DaemonEvent},
    messages::{Message, RequestMessage, ResponseMessage},
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

pub async fn send_and_recv<Req, Res>(stream: &mut UnixStream, message: &Req) -> Result<Res>
where
    Req: Message + Serialize,
    Res: Message + DeserializeOwned,
{
    write_message(stream, message).await?;
    read_message(stream).await
}

/// writes a serializable message and returns the request id
pub async fn write_message<M>(stream: &mut UnixStream, message: &M) -> Result<u32>
where
    M: Message + Serialize,
{
    let bytes = serde_json::to_vec(message)?;
    let num_bytes = bytes.len() as u32;

    let _written = stream.write(&num_bytes.to_be_bytes()).await?;
    let _written = stream.write(&bytes).await?;
    Ok(message.get_id())
}

pub async fn read_message<M>(stream: &mut UnixStream) -> Result<M>
where
    M: Message + DeserializeOwned,
{
    let mut num_bytes = [0u8; 4];
    stream.read_exact(&mut num_bytes).await?;
    let num_bytes = u32::from_be_bytes(num_bytes);

    let mut message_bytes = vec![0u8; num_bytes as usize];
    stream.read_exact(&mut message_bytes).await?;

    Ok(serde_json::from_slice(&message_bytes)?)
}

pub async fn read_req(stream: &mut UnixStream) -> Result<RequestMessage> {
    read_message(stream).await
}

pub async fn read_res(stream: &mut UnixStream) -> Result<ResponseMessage> {
    read_message(stream).await
}

#[cfg(test)]
mod test {
    #![allow(clippy::unwrap_used)]
    use std::{fs::remove_file, path::PathBuf};

    use tokio::net::UnixListener;

    use super::*;
    use crate::{constants::TEMP_SOCK_DIR, messages::RequestBody};

    #[tokio::test]
    async fn test_tcp_message() -> Result<()> {
        // Bind server
        let temp_dir = PathBuf::from(TEMP_SOCK_DIR);
        if temp_dir.exists() {
            remove_file(&temp_dir)?;
        }

        let listener = UnixListener::bind(temp_dir)?;
        let addr = listener.local_addr()?;

        // Spawn server
        let server: tokio::task::JoinHandle<Result<()>> = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await?;
            let msg1 = read_req(&mut socket).await?;
            assert_eq!(
                msg1,
                RequestMessage {
                    id: msg1.get_id(),
                    body: RequestBody::Attach { session_id: 1 }
                }
            );

            Ok(())
        });

        // Connect client
        let mut client = UnixStream::connect(addr.as_pathname().unwrap())
            .await
            .unwrap();
        write_message(
            &mut client,
            &RequestMessage::body(RequestBody::Attach { session_id: 1 }),
        )
        .await
        .unwrap();
        server.await.unwrap()?;

        Ok(())
    }
}

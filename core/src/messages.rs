use derive_more::Display;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};

pub use crate::error::{Error, Result};

#[derive(Serialize, Deserialize, Debug, PartialEq, Display)]
pub enum RemuxDaemonRequest {
    #[display("Connect: {{session_id: {session_id}, create: {create}}}")]
    Connect {
        session_id: u16,
        create: bool,
    },
    Disconnect,

    // pane commands
    NewPane,
    CyclePane, // probably temporary
    KillPane,
}

pub enum RemuxDaemonResponse {}

pub async fn write_message<T: Serialize>(stream: &mut UnixStream, message: &T) -> Result<()> {
    let bytes = serde_json::to_vec(message)?;
    let num_bytes = bytes.len() as u32;

    let _written = stream.write(&num_bytes.to_be_bytes()).await?;
    let _written = stream.write(&bytes).await?;
    Ok(())
}

pub async fn read_message<R, T>(reader: &mut R) -> Result<T>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let mut num_bytes = [0u8; 4];
    reader.read_exact(&mut num_bytes).await?;
    let num_bytes = u32::from_be_bytes(num_bytes);

    let mut message_bytes = vec![0u8; num_bytes as usize];
    reader.read_exact(&mut message_bytes).await?;

    let message: T = serde_json::from_slice(&message_bytes)?;
    Ok(message)
}

#[cfg(test)]
mod test {
    #![allow(clippy::unwrap_used)]
    use std::{fs::remove_file, path::PathBuf};

    use tokio::net::UnixListener;

    use super::*;
    use crate::constants::TEMP_SOCK_DIR;

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
            let msg1: RemuxDaemonRequest = read_message(&mut socket).await?;
            assert_eq!(msg1, RemuxDaemonRequest::Connect);
            let msg2: RemuxDaemonRequest = read_message(&mut socket).await?;
            assert_eq!(msg2, RemuxDaemonRequest::Disconnect);

            Ok(())
        });

        // Connect client
        let mut client = UnixStream::connect(addr.as_pathname().unwrap())
            .await
            .unwrap();
        write_message(&mut client, &RemuxDaemonRequest::Connect)
            .await
            .unwrap();
        write_message(&mut client, &RemuxDaemonRequest::Disconnect)
            .await
            .unwrap();
        server.await.unwrap()?;

        Ok(())
    }
}

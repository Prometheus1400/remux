use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use crate::error::RemuxLibError;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum RemuxDaemonRequest {
    Connect,
    Disconnect,
}

pub enum RemuxDaemonResponse {}

pub async fn write_message<T: Serialize>(
    stream: &mut TcpStream,
    message: T,
) -> Result<(), RemuxLibError> {
    let bytes = serde_json::to_vec(&message).unwrap();
    let num_bytes = bytes.len() as u32;

    let _written = stream.write(&num_bytes.to_be_bytes()).await?;
    let _written = stream.write(&bytes).await?;
    Ok(())
}

pub async fn read_message<R, T>(reader: &mut R) -> Result<T, RemuxLibError>
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
    use super::*;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn test_tcp_message() {
        // Bind server
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Spawn server
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let msg1: RemuxDaemonRequest = read_message(&mut socket).await.unwrap();
            assert_eq!(msg1, RemuxDaemonRequest::Connect);
            let msg2: RemuxDaemonRequest = read_message(&mut socket).await.unwrap();
            assert_eq!(msg2, RemuxDaemonRequest::Disconnect);
        });

        // Connect client
        let mut client = TcpStream::connect(addr).await.unwrap();
        write_message(&mut client, RemuxDaemonRequest::Connect)
            .await
            .unwrap();
        write_message(&mut client, RemuxDaemonRequest::Disconnect)
            .await
            .unwrap();
        server.await.unwrap();
    }
}

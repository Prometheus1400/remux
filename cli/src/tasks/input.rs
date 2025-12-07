use bytes::Bytes;
use tokio::{
    io::AsyncReadExt,
    signal::unix::{SignalKind, signal},
    sync::mpsc,
};

use crate::prelude::*;

#[derive(Debug)]
pub enum Input {
    Stdin(Bytes),
    Resize,
}

pub fn start_input_listeners(tx: mpsc::Sender<Input>) -> Vec<CliTask> {
    let task1: CliTask = tokio::spawn({
        let tx = tx.clone();
        async move {
            let mut stdin = tokio::io::stdin();
            let mut buf = [0u8; 1024];
            loop {
                match stdin.read(&mut buf).await {
                    Ok(n) if n > 0 => {
                        trace!("read {} bytes from stdin", n);
                        tx.send(Input::Stdin(Bytes::copy_from_slice(&buf[..n]))).await.unwrap();
                    }
                    Ok(_) => {
                        break;
                    }
                    Err(e) => {
                        error!("Error receiving stdin: {e}");
                        continue;
                    }
                }
            }
            Ok(())
        }
    });

    let task2: CliTask = tokio::spawn(async move {
        let mut sigwinch = signal(SignalKind::window_change()).unwrap();
        while sigwinch.recv().await.is_some() {
            tx.send(Input::Resize).await.unwrap();
        }
        Ok(())
    });

    vec![task1, task2]
}

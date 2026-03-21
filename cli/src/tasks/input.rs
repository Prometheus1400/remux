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
                        if tx.send(Input::Stdin(Bytes::copy_from_slice(&buf[..n]))).await.is_err() {
                            debug!("stdin listener exiting because input channel closed");
                            break;
                        }
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
        let mut sigwinch = signal(SignalKind::window_change())?;
        while sigwinch.recv().await.is_some() {
            if tx.send(Input::Resize).await.is_err() {
                debug!("resize listener exiting because input channel closed");
                break;
            }
        }
        Ok(())
    });

    vec![task1, task2]
}

use bytes::Bytes;
use crossterm::event::{self, Event};
use tokio::{io::AsyncReadExt, sync::mpsc};

use crate::prelude::*;

#[derive(Debug)]
pub enum Input {
    Stdin(Bytes),
    Crossterm(Event),
}

pub fn start_input_listener(tx: mpsc::Sender<Input>) {
    tokio::spawn({
        let tx = tx.clone();
        async move {
            let mut stdin = tokio::io::stdin();
            let mut buf = [0u8; 1024];
            loop {
                match stdin.read(&mut buf).await {
                    Ok(n) if n > 0 => {
                        tx.send(Input::Stdin(Bytes::copy_from_slice(&buf))).await.unwrap();
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
        }
    });

    tokio::spawn(async move {
        loop {
            if event::poll(std::time::Duration::from_millis(100)).unwrap() {
                let ev = event::read().unwrap();
                tx.send(Input::Crossterm(ev)).await.ok();
            }
        }
    });
}

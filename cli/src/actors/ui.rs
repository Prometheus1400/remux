use std::io::{Stdout, stdout};

use bytes::Bytes;
use ratatui::{Terminal, prelude::CrosstermBackend};
use tokio::sync::mpsc;
use tracing::Instrument;

use crate::{actors::client::ClientHandle, prelude::*, widgets};

#[derive(Debug)]
pub enum UiEvent {
    SelectSession { session_ids: Vec<u32> },
    Kill,
}
use UiEvent::*;

#[derive(Debug)]
pub struct Ui {
    handle: UiHandle,
    rx: mpsc::Receiver<UiEvent>,
    client_handle: ClientHandle,
    terminal: Terminal<CrosstermBackend<Stdout>>,
    stdin_rx: mpsc::Receiver<Bytes>,
}
impl Ui {
    #[instrument(skip(stdin_rx, client_handle))]
    pub fn spawn(stdin_rx: mpsc::Receiver<Bytes>, client_handle: ClientHandle) -> Result<UiHandle> {
        Ui::new(stdin_rx, client_handle)?.run()
    }

    #[instrument(skip(stdin_rx, client_handle))]
    fn new(stdin_rx: mpsc::Receiver<Bytes>, client_handle: ClientHandle) -> Result<Self> {
        let (tx, rx) = mpsc::channel(10);
        let handle = UiHandle { tx };
        let terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
        Ok(Self {
            stdin_rx,
            handle,
            rx,
            client_handle,
            terminal,
        })
    }

    #[instrument(skip(self))]
    fn run(mut self) -> Result<UiHandle> {
        let span = tracing::Span::current();
        let handle_clone = self.handle.clone();
        tokio::spawn({
            async move {
                loop {
                    if let Some(event) = self.rx.recv().await {
                        match event {
                            SelectSession { session_ids } => {
                                debug!("SelectSession");
                                self.handle_select_session(session_ids).await?;
                            }
                            Kill => {
                                debug!("Kill");
                                break;
                            }
                        }
                    }
                }
                debug!("Popup actor stopping");
                Result::Ok(())
            }
            .instrument(span)
        });

        Ok(handle_clone)
    }

    async fn handle_select_session(&mut self, session_ids: Vec<u32>) -> Result<()> {
        let session_id_strs: Vec<String> = session_ids.iter().map(|i| i.to_string()).collect();
        if let Some(index) = widgets::selector_widget(
            &mut self.terminal,
            &mut self.stdin_rx,
            &session_id_strs,
            "Select Session",
        )
        .await
        {
            self.client_handle
                .send_switch_session(session_ids.get(index).copied())
                .await?;
        } else {
            self.client_handle.send_switch_session(None).await?
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct UiHandle {
    tx: mpsc::Sender<UiEvent>,
}
impl UiHandle {
    pub async fn send_select_session(&mut self, session_ids: Vec<u32>) -> Result<()> {
        Ok(self.tx.send(SelectSession { session_ids }).await?)
    }
    pub async fn send_kill(&mut self) -> Result<()> {
        Ok(self.tx.send(Kill).await?)
    }
}

use std::{
    fmt::Display,
    io::{Stdout, stdout},
    ops::Index,
};

use bytes::Bytes;
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    Terminal,
    prelude::CrosstermBackend,
    style::{Style, Stylize},
    widgets::{List, ListItem},
};
use serde::de;
use tokio::sync::mpsc::{self, Receiver};
use tracing::debug;

use crate::{actors::io::IOHandle, prelude::*, widgets};

#[derive(Debug)]
pub enum PopupEvent {
    SelectSession { session_ids: Vec<u32> },
    Kill,
}
use PopupEvent::*;

#[derive(Debug)]
pub struct Popup {
    handle: PopupHandle,
    rx: mpsc::Receiver<PopupEvent>,
    io_handle: IOHandle,
    terminal: Terminal<CrosstermBackend<Stdout>>,
    stdin_rx: mpsc::Receiver<Bytes>,
}
impl Popup {
    pub fn spawn(stdin_rx: mpsc::Receiver<Bytes>, io_handle: IOHandle) -> Result<PopupHandle> {
        Popup::new(stdin_rx, io_handle).run()
    }

    fn new(stdin_rx: mpsc::Receiver<Bytes>, io_handle: IOHandle) -> Self {
        let (tx, rx) = mpsc::channel(10);
        let handle = PopupHandle { tx };
        let terminal = Terminal::new(CrosstermBackend::new(stdout())).unwrap();
        Self {
            stdin_rx,
            handle,
            rx,
            io_handle,
            terminal,
        }
    }

    fn run(mut self) -> Result<PopupHandle> {
        let handle_clone = self.handle.clone();
        let _: CliTask = tokio::spawn({
            async move {
                loop {
                    if let Some(event) = self.rx.recv().await {
                        match event {
                            SelectSession { session_ids } => {
                                self.handle_select_session(session_ids).await.unwrap();
                            }
                            Kill => {
                                break;
                            }
                        }
                    }
                }
                debug!("Popup actor stopping");
            }
        });

        Ok(handle_clone)
    }

    async fn handle_select_session(&mut self, session_ids: Vec<u32>) -> Result<()> {
        let session_id_strs: Vec<String> = session_ids.iter().map(|i| i.to_string()).collect();
        if let Some(index) =
            widgets::selector::selector_widget(&mut self.terminal, &mut self.stdin_rx, &session_id_strs).await
        {
            self.io_handle
                .send_switch_session(session_ids.get(index).copied())
                .await?;
        } else {
            self.io_handle.send_switch_session(None).await?
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct PopupHandle {
    tx: mpsc::Sender<PopupEvent>,
}
impl PopupHandle {
    pub async fn send_select_session(&mut self, session_ids: Vec<u32>) -> Result<()> {
        Ok(self.tx.send(SelectSession { session_ids }).await?)
    }
    pub async fn send_kill(&mut self) -> Result<()> {
        Ok(self.tx.send(Kill).await?)
    }
}

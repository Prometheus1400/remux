use bytes::Bytes;
use remux_core::{
    communication,
    events::{CliEvent, DaemonEvent},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    sync::mpsc,
};
use tracing::{Instrument, debug};

use crate::{
    actors::{WidgetRunner, widget_runner::WidgetRunnerHandle},
    prelude::*,
};

#[derive(Debug)]
pub enum ClientEvent {
    SwitchSession { session_id: Option<u32> },
}
use ClientEvent::*;

#[derive(Debug)]
enum StdinState {
    Daemon,
    Popup,
}

#[derive(Debug)]
enum DaemonEventsState {
    Blocked,
    Unblocked,
}

#[derive(Debug)]
pub struct Client {
    stream: UnixStream,              // the client owns the stream
    handle: ClientHandle,            // handle used to send the client events
    rx: mpsc::Receiver<ClientEvent>, // receiver for client events
    // ui_handle: UiHandle,                    // handle used to send the popup actor events
    stdin_state: StdinState, // for routing stdin to daemon or popup actor
    daemon_events_state: DaemonEventsState, // determines if currently accepting events from daemon
    stdin_tx: mpsc::Sender<Bytes>, // this is for popup actor to connect to stdin
    widget_runner: WidgetRunnerHandle,
}
impl Client {
    #[instrument(skip(stream))]
    pub fn spawn(stream: UnixStream) -> Result<CliTask> {
        Client::new(stream)?.run()
    }

    #[instrument(skip(stream))]
    fn new(stream: UnixStream) -> Result<Self> {
        let (tx, rx) = mpsc::channel(100);
        let (stdin_tx, stdin_rx) = mpsc::channel(100);
        let handle = ClientHandle { tx };
        let widget_runner = WidgetRunner::spawn(stdin_rx, handle.clone())?;
        Ok(Self {
            stream,
            handle,
            rx,
            stdin_state: StdinState::Daemon,
            daemon_events_state: DaemonEventsState::Unblocked,
            stdin_tx,
            widget_runner,
        })
    }

    #[instrument(skip(self), fields(stdin_state = ?self.stdin_state,  daemon_events_state = ?self.daemon_events_state))]
    fn run(mut self) -> Result<CliTask> {
        let task: CliTask = tokio::spawn({
            let span = tracing::Span::current();
            let mut stdin = tokio::io::stdin();
            let mut stdin_buf = [0u8; 1024];
            let mut stdout = tokio::io::stdout();
            async move {
                loop {
                    tokio::select! {
                        Some(event) = self.rx.recv() => {
                            match event {
                                SwitchSession {session_id} => {
                                    debug!("SwitchSession({session_id:?}))");
                                    if let Some(session_id) = session_id {
                                        communication::send_event(&mut self.stream, CliEvent::SwitchSession { session_id }).await.unwrap();
                                    }
                                    self.daemon_events_state = DaemonEventsState::Unblocked;
                                    self.stdin_state = StdinState::Daemon;
                                }
                            }
                        },
                        res = communication::recv_daemon_event(&mut self.stream), if matches!(self.daemon_events_state, DaemonEventsState::Unblocked) => {
                            match res {
                                Ok(event) => {
                                    match event {
                                        DaemonEvent::Raw{bytes} => {
                                            trace!("DaemonEvent(Raw({bytes:?}))");
                                            stdout.write_all(&bytes).await?;
                                            stdout.flush().await?;
                                        }
                                        DaemonEvent::SwitchSessionOptions{session_ids} => {
                                            debug!("DaemonEvent(SwitchSessionOptions({session_ids:?}))");
                                            self.daemon_events_state = DaemonEventsState::Blocked;
                                            self.stdin_state = StdinState::Popup;
                                            self.widget_runner.select_session(session_ids).await?;
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("Error receiving daemon event: {e}");
                                    break;
                                }
                            }
                        }
                        stdin_res = stdin.read(&mut stdin_buf) => {
                            match stdin_res {
                                Ok(n) if n > 0 => {
                                    match self.stdin_state {
                                        StdinState::Daemon => {
                                            trace!("Sending {n} bytes to Daemon");
                                            communication::send_event(&mut self.stream, CliEvent::Raw{bytes: stdin_buf[..n].to_vec() }).await?;

                                        },
                                        StdinState::Popup => {
                                            trace!("Sending {n} bytes to Popup");
                                            self.stdin_tx.send(Bytes::copy_from_slice(&stdin_buf[..n])).await?;
                                        },
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
                        },
                    }
                }
                debug!("Client stopped");
                Ok(())
            }.instrument(span)
        });

        Ok(task)
    }
}

#[derive(Debug, Clone)]
pub struct ClientHandle {
    tx: mpsc::Sender<ClientEvent>,
}
impl ClientHandle {
    pub async fn send_switch_session(&mut self, session_id: Option<u32>) -> Result<()> {
        Ok(self
            .tx
            .send(ClientEvent::SwitchSession { session_id })
            .await?)
    }
}

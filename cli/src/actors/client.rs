use std::time::Duration;

use bytes::Bytes;
use handle_macro::Handle;
use remux_core::{
    communication,
    events::{CliEvent, DaemonEvent},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    sync::mpsc,
    time::interval,
};
use tracing::{Instrument, debug};

use crate::{
    actors::ui::{UI, UIHandle},
    input_parser::{Action, InputParser, ParsedEvent},
    prelude::*,
    states::daemon_state::DaemonState,
};

#[derive(Handle)]
pub enum ClientEvent {
    Selected(Option<usize>), // index of the selected item
}
use ClientEvent::*;

#[derive(Debug)]
enum ClientState {
    Normal,
    SelectingSession,
}

#[derive(Debug)]
pub struct Client {
    stream: UnixStream,               // the client owns the stream
    handle: ClientHandle,             // handle used to send the client events
    rx: mpsc::Receiver<ClientEvent>,  // receiver for client events
    daemon_state: DaemonState,        // determines if currently accepting events from daemon
    ui_stdin_tx: mpsc::Sender<Bytes>, // this is for popup actor to connect to stdin
    ui_handle: UIHandle,
    input_parser: InputParser,
    client_state: ClientState,
}
impl Client {
    #[instrument(skip(stream))]
    pub fn spawn(stream: UnixStream) -> Result<CliTask> {
        Client::new(stream)?.run()
    }

    #[instrument(skip(stream))]
    fn new(stream: UnixStream) -> Result<Self> {
        let (tx, rx) = mpsc::channel(100);
        let (ui_stdin_tx, ui_stdin_rx) = mpsc::channel(100);
        let handle = ClientHandle { tx };
        let ui_handle = UI::spawn(handle.clone(), ui_stdin_rx)?;
        Ok(Self {
            stream,
            handle,
            rx,
            ui_stdin_tx,
            ui_handle,
            daemon_state: DaemonState::default(),
            input_parser: InputParser::new(),
            client_state: ClientState::Normal,
        })
    }

    #[instrument(skip(self), fields(client_state = ?self.client_state))]
    fn run(mut self) -> Result<CliTask> {
        let task: CliTask = tokio::spawn({
            let span = tracing::Span::current();
            let mut stdin = tokio::io::stdin();
            let mut stdin_buf = [0u8; 1024];
            async move {
                let mut ticker = interval(Duration::from_millis(1000));
                loop {
                    tokio::select! {
                        Some(event) = self.rx.recv() => {
                            match event {
                                Selected(index) => {
                                    debug!("Selected: {index:?}");
                                    match self.client_state {
                                        ClientState::Normal => {
                                            error!("should not receive selected event in normal state");
                                        },
                                        ClientState::SelectingSession => {
                                            if let Some(index) = index {
                                                let selected_session = self.daemon_state.session_ids[index];
                                                debug!("sending session selection: {selected_session}");
                                                communication::send_event(&mut self.stream, CliEvent::SwitchSession { session_id: selected_session  }).await.unwrap();
                                            }
                                            debug!("Returning to normal state");
                                            self.client_state = ClientState::Normal;
                                        }
                                    }
                                }
                            }
                        },
                        res = communication::recv_daemon_event(&mut self.stream), if matches!(self.client_state, ClientState::Normal) => {
                            match res {
                                Ok(event) => {
                                    match event {
                                        DaemonEvent::Raw{bytes} => {
                                            trace!("DaemonEvent(Raw({bytes:?}))");
                                            self.ui_handle.output(Bytes::from(bytes)).await?;
                                        }
                                        DaemonEvent::Disconnected => {
                                            debug!("DaemonEvent(Disconnected)");
                                            self.ui_handle.kill().await.unwrap();
                                            break;
                                        }
                                        DaemonEvent::SwitchSessionOptions{session_ids} => {
                                            debug!("DaemonEvent(SwitchSessionOptions({session_ids:?}))");
                                            self.client_state = ClientState::SelectingSession;
                                            // self.widget_runner_handle.select_session(session_ids).await?;
                                        }
                                        DaemonEvent::CurrentSessions(session_ids) => {
                                            debug!("DaemonEvent(CurrentSessions({session_ids:?}))");
                                            self.daemon_state.set_sessions(session_ids);
                                        }
                                        DaemonEvent::ActiveSession(session_id) => {
                                            debug!("DaemonEvent(ActiveSession({session_id}))");
                                            self.daemon_state.set_active_session(session_id);
                                        }
                                        DaemonEvent::NewSession(session_id) => {
                                            debug!("DaemonEvent(NewSession({session_id}))");
                                            self.daemon_state.add_session(session_id);
                                        }
                                        DaemonEvent::DeletedSession(session_id) => {
                                            todo!("implement delete session");
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
                                    match self.client_state {
                                        ClientState::Normal => {
                                            trace!("Sending {n} bytes to Daemon");
                                            for event in self.input_parser.process(&stdin_buf[..n]) {
                                                match event {
                                                    ParsedEvent::DaemonAction(cli_event) => {
                                                        communication::send_event(&mut self.stream, cli_event).await?;
                                                    },
                                                    ParsedEvent::LocalAction(local_action) => {
                                                        match local_action {
                                                            Action::SwitchSession => {
                                                                self.client_state = ClientState::SelectingSession;
                                                                let items: Vec<Box<dyn ToString + Send + Sync>> = self.daemon_state.session_ids.iter().copied().map(|x| Box::new(x) as Box<dyn ToString + Send + Sync>).collect();
                                                                self.ui_handle.select(items).await.unwrap();
                                                                // self.widget_runner_handle.select_session(self.daemon_state.session_ids.clone()).await?;
                                                            },
                                                        }
                                                    },
                                                }
                                            }

                                        },
                                        ClientState::SelectingSession => {
                                            trace!("Sending {n} bytes to ui");
                                            self.ui_stdin_tx.send(Bytes::copy_from_slice(&stdin_buf[..n])).await?;
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
                        _ = ticker.tick() => {
                            // TODO: make this event driven instead of on timer
                            self.ui_handle.sync_daemon_state(self.daemon_state.clone()).await?;
                        }
                    }
                }
                debug!("Client stopped");
                Ok(())
            }.instrument(span)
        });

        Ok(task)
    }
}

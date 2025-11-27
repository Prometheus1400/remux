use std::time::Duration;

use bytes::Bytes;
use handle_macro::Handle;
use remux_core::{
    communication,
    events::{CliEvent, DaemonEvent},
};
use tokio::{io::AsyncReadExt, net::UnixStream, sync::mpsc, time::interval};
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
    Normal,           // running normally with stdin parsed into events and sent to daemon
    SelectingSession, // means that the ui is currently busy selecting redirects stdin to ui selector
}

#[derive(Debug)]
pub struct Client {
    _handle: ClientHandle,            // handle used to send the client events
    stream: UnixStream,               // the client owns the stream
    rx: mpsc::Receiver<ClientEvent>,  // receiver for client events
    daemon_state: DaemonState,        // determines if currently accepting events from daemon
    sync_daemon_state: bool,          // if the state is dirty only then do we need to sync to the ui
    ui_stdin_tx: mpsc::Sender<Bytes>, // this is for popup actor to connect to stdin
    ui_handle: UIHandle,              // how the client sends messages to ui
    input_parser: InputParser,        // converts streams of bytes into actionable events
    client_state: ClientState,        // the current state of the client
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
            _handle: handle,
            stream,
            rx,
            ui_stdin_tx,
            ui_handle,
            daemon_state: DaemonState::default(),
            sync_daemon_state: false,
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
                        res = communication::recv_daemon_event(&mut self.stream) => {
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
                                        DaemonEvent::CurrentSessions(session_ids) => {
                                            debug!("DaemonEvent(CurrentSessions({session_ids:?}))");
                                            self.daemon_state.set_sessions(session_ids);
                                            self.sync_daemon_state = true;
                                        }
                                        DaemonEvent::ActiveSession(session_id) => {
                                            debug!("DaemonEvent(ActiveSession({session_id}))");
                                            self.daemon_state.set_active_session(session_id);
                                            self.sync_daemon_state = true;
                                        }
                                        DaemonEvent::NewSession(session_id) => {
                                            debug!("DaemonEvent(NewSession({session_id}))");
                                            self.daemon_state.add_session(session_id);
                                            self.sync_daemon_state = true;
                                        }
                                        DaemonEvent::DeletedSession(_session_id) => {
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
                                                                self.ui_handle.select(items, "Select Session".to_owned()).await.unwrap();
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
                        _ = ticker.tick(), if self.sync_daemon_state => {
                            debug!("syncing daemon_state");
                            self.ui_handle.sync_daemon_state(self.daemon_state.clone()).await?;
                            self.sync_daemon_state = false;
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

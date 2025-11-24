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
    actors::{
        WidgetRunner,
        ui::{UI, UIHandle},
        widget_runner::WidgetRunnerHandle,
    },
    input_parser::{Action, InputParser, ParsedEvent},
    prelude::*,
    state_view::StateView,
};

#[derive(Handle)]
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
    stdin_state: StdinState,                // for routing stdin to daemon or popup actor
    daemon_events_state: DaemonEventsState, // determines if currently accepting events from daemon
    stdin_tx: mpsc::Sender<Bytes>,          // this is for popup actor to connect to stdin
    widget_runner_handle: WidgetRunnerHandle,
    ui_handle: UIHandle,
    state_view: StateView,
    input_parser: InputParser,
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
        let widget_runner_handle = WidgetRunner::spawn(stdin_rx, handle.clone())?;
        let ui_handle = UI::spawn()?;
        Ok(Self {
            stream,
            handle,
            rx,
            stdin_state: StdinState::Daemon,
            daemon_events_state: DaemonEventsState::Unblocked,
            stdin_tx,
            widget_runner_handle,
            ui_handle,
            state_view: StateView::default(),
            input_parser: InputParser::new(),
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
                let mut ticker = interval(Duration::from_millis(1000));
                loop {
                    tokio::select! {
                        Some(event) = self.rx.recv() => {
                            match event {
                                SwitchSession {session_id} => {
                                    debug!("SwitchSession({session_id:?}))");
                                    if let Some(session_id) = session_id {
                                        communication::send_event(&mut self.stream, CliEvent::SwitchSession { session_id }).await.unwrap();
                                    }
                                    self.ui_handle.clear_terminal().await?;
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
                                            self.ui_handle.output(Bytes::from(bytes)).await?;
                                        }
                                        DaemonEvent::SwitchSessionOptions{session_ids} => {
                                            debug!("DaemonEvent(SwitchSessionOptions({session_ids:?}))");
                                            self.daemon_events_state = DaemonEventsState::Blocked;
                                            self.stdin_state = StdinState::Popup;
                                            self.widget_runner_handle.select_session(session_ids).await?;
                                        }
                                        DaemonEvent::CurrentSessions(session_ids) => {
                                            debug!("DaemonEvent(CurrentSessions({session_ids:?}))");
                                            self.state_view.set_sessions(session_ids);
                                        }
                                        DaemonEvent::ActiveSession(session_id) => {
                                            debug!("DaemonEvent(ActiveSession({session_id}))");
                                            self.state_view.set_active_session(session_id);
                                        }
                                        DaemonEvent::NewSession(session_id) => {
                                            debug!("DaemonEvent(NewSession({session_id}))");
                                            self.state_view.add_session(session_id);
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
                                    match self.stdin_state {
                                        StdinState::Daemon => {
                                            trace!("Sending {n} bytes to Daemon");
                                            for event in self.input_parser.process(&stdin_buf[..n]) {
                                                match event {
                                                    ParsedEvent::DaemonAction(cli_event) => {
                                                        communication::send_event(&mut self.stream, cli_event).await?;
                                                    },
                                                    ParsedEvent::LocalAction(local_action) => {
                                                        match local_action {
                                                            Action::SwitchSession => {
                                                                self.daemon_events_state = DaemonEventsState::Blocked;
                                                                self.stdin_state = StdinState::Popup;
                                                                self.widget_runner_handle.select_session(self.state_view.session_ids.clone()).await?;
                                                            },
                                                        }
                                                    },
                                                }
                                            }

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
                        _ = ticker.tick() => {
                            // TODO: make this event driven instead of on timer
                            self.ui_handle.sync_state_view(self.state_view.clone()).await?;
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

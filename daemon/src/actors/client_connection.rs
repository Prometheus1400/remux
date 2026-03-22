use std::sync::Arc;

use bytes::Bytes;
use color_eyre::eyre::WrapErr;
use handle_macro::Handle;
use remux_core::{
    comm,
    config::BuiltinAction,
    events::{CliEvent, DaemonEvent},
    messages::{ResponseBuilder, ResponseResult, response},
};
use tokio::{net::UnixStream, sync::mpsc};
use uuid::Uuid;

use crate::{
    actors::session_manager::SessionManagerHandle,
    input_parser::{InputParser, ParsedInput},
    layout::SplitDirection,
    actors::window::FocusDirection,
    lua::config::ConfigRuntime,
    prelude::*,
};

#[allow(unused)]
#[derive(Handle, Debug)]
pub enum ClientConnectionEvent {
    // AttachToSession(u32),
    SuccessAttachToSession(u32),
    FailedAttachToSession(u32),
    DetachFromSession(u32),
    SessionOutput(Bytes),
    Disconnect,

    // client side state update events
    // variants related to initialization phase
    InitialAttach(u32), // invoked directly by the daemon
    // this variant is unique in that it responds to client by sending a message not an event
    InitialAttachResult(Result<()>),
}
use ClientConnectionEvent::*;

#[allow(unused)]
#[derive(Debug)]
enum ClientConnectionState {
    Unattached,
    Attached,
}

pub struct ClientConnection {
    id: Uuid,
    request_id: u32,
    stream: UnixStream,
    handle: ClientConnectionHandle,
    rx: mpsc::Receiver<ClientConnectionEvent>,
    session_manager_handle: SessionManagerHandle,
    config_runtime: Arc<ConfigRuntime>,
    input_parser: InputParser,
    state: ClientConnectionState,
}
impl ClientConnection {
    pub fn spawn(
        request_id: u32,
        id: Uuid,
        stream: UnixStream,
        session_manager_handle: SessionManagerHandle,
        config_runtime: Arc<ConfigRuntime>,
        initial_session_name: &str,
        rows: u16,
        cols: u16,
    ) -> Result<ClientConnectionHandle> {
        let client = Self::new(request_id, id, stream, session_manager_handle, config_runtime);
        client.run(initial_session_name, rows, cols)
    }
    fn new(
        request_id: u32,
        id: Uuid,
        stream: UnixStream,
        session_manager_handle: SessionManagerHandle,
        config_runtime: Arc<ConfigRuntime>,
    ) -> Self {
        let (tx, rx) = mpsc::channel(10);
        let handle = ClientConnectionHandle { tx };

        Self {
            id,
            request_id,
            stream,
            handle,
            rx,
            session_manager_handle,
            input_parser: InputParser::new(config_runtime.key_bindings()),
            config_runtime,
            state: ClientConnectionState::Unattached,
        }
    }
    fn run(mut self, initial_session_name: &str, rows: u16, cols: u16) -> Result<ClientConnectionHandle> {
        let handle_clone = self.handle.clone();
        let session_name = initial_session_name.to_owned();
        let client_id = self.id;
        let _task = tokio::spawn(
            async move {
                let handle = self.handle.clone();
                self.session_manager_handle
                    .client_connect(self.id, handle.clone(), Some(session_name), true, rows, cols)
                    .await
                    .wrap_err("failed to register client with session manager")?;
                loop {
                    tokio::select! {
                        Some(event) = self.rx.recv() => {
                            let span = error_span!("Recieved Client Connection Event");
                            let _guard = span.enter();
                            match &event {
                                SessionOutput(bytes) => {
                                    trace!(event=?event, num_bytes=bytes.len());
                                }
                                _ => {
                                    info!(event=?event);
                                }
                            }
                            let should_break = match event {
                                InitialAttachResult(result) if matches!(self.state, ClientConnectionState::Unattached) => {
                                    match result {
                                        Ok(server_snapshot) => {
                                            let _ = server_snapshot;
                                            let res = ResponseBuilder::default()
                                                .id(self.request_id)
                                                .result(ResponseResult::Success(response::Attach {
                                                    attached: true,
                                                }))
                                                .build();
                                            info!(respnse=?res, "Sending response");
                                            if let Err(e) = comm::send_message(&mut self.stream, &res).await {
                                                warn!(error=%e, client_id=%self.id, "Failed to send initial attach response");
                                                true
                                            } else {
                                                self.state = ClientConnectionState::Attached;
                                                false
                                            }
                                        }
                                        Err(e) => {
                                            let response = ResponseBuilder::default()
                                                .id(self.request_id)
                                                .result(ResponseResult::Failure::<()> {
                                                    message: e.to_string(),
                                                })
                                                .build();
                                            if let Err(send_err) = comm::send_message(&mut self.stream, &response).await {
                                                warn!(error=%send_err, client_id=%self.id, "Failed to send initial attach failure");
                                                true
                                            } else {
                                                false
                                            }
                                        }
                                    }
                                }
                                SuccessAttachToSession(session_id) => {
                                    let _ = session_id;
                                    self.state = ClientConnectionState::Attached;
                                    false
                                }
                                FailedAttachToSession(..) => {
                                    self.send_daemon_event(DaemonEvent::Disconnected).await.is_err()
                                }
                                DetachFromSession(..) => {
                                    self.state = ClientConnectionState::Unattached;
                                    false
                                }
                                Disconnect => {
                                    let _ = self.send_daemon_event(DaemonEvent::Disconnected).await;
                                    true
                                }
                                SessionOutput(bytes) => {
                                    self.send_session_output(bytes).await.is_err()
                                }
                                _ => {
                                    error!(event=?event, state=?self.state, "Unhandled or invalid event for current state");
                                    false
                                }
                            };

                            if should_break {
                                break;
                            }
                        },
                        res = comm::recv_cli_event(&mut self.stream), if matches!(self.state, ClientConnectionState::Attached) => {
                            match res {
                                Ok(event) => {
                                    let span = error_span!("Recieved Cli Event", event=?event);
                                    let _guard = span.enter();
                                    match &event {
                                        CliEvent::Raw(..) => {
                                            trace!(event=?event);
                                        }
                                        _ => {
                                            info!(event=?event);
                                        }
                                    }
                                    let result = match event {
                                        CliEvent::Raw(bytes) => self.handle_stdin(bytes).await,
                                        CliEvent::TerminalResize{rows, cols} => {
                                            self.session_manager_handle.terminal_resize(rows, cols).await
                                        },
                                        CliEvent::Detach => {
                                            self.detach_and_exit().await?;
                                            break;
                                        },
                                    };

                                    if let Err(e) = result {
                                        error!(error=%e, client_id=%self.id, "Failed to handle client event");
                                    }
                                }
                                Err(e) => {
                                    // client disconnected
                                    debug!("Client disconnected because of error recieving cli event: {e}");
                                    self.cleanup_after_disconnect();
                                    break;
                                }
                            }
                        }
                    }
                }
                Ok::<(), Error>(())
            }
            .instrument(error_span!(parent: None, "Client Actor", id=?client_id))
        );

        Ok(handle_clone)
    }

    async fn send_daemon_event(&mut self, event: DaemonEvent) -> Result<()> {
        comm::send_event(&mut self.stream, event).await.map_err(Into::into)
    }

    async fn send_session_output(&mut self, bytes: Bytes) -> Result<()> {
        let chunk_size = 1024;
        for chunk in bytes.chunks(chunk_size) {
            self.send_daemon_event(DaemonEvent::Raw(Bytes::copy_from_slice(chunk)))
                .await?;
        }
        Ok(())
    }

    async fn detach_and_exit(&mut self) -> Result<()> {
        self.state = ClientConnectionState::Unattached;
        self.send_daemon_event(DaemonEvent::Disconnected).await?;
        self.cleanup_after_disconnect();
        Ok(())
    }

    fn cleanup_after_disconnect(&self) {
        let session_manager_handle = self.session_manager_handle.clone();
        let client_id = self.id;
        tokio::spawn(async move {
            if let Err(disconnect_err) = session_manager_handle.client_disconnect(client_id).await {
                warn!(error=%disconnect_err, client_id=%client_id, "Failed to notify session manager about disconnect");
            }
        });
    }

    async fn handle_stdin(&mut self, bytes: Bytes) -> Result<()> {
        for input in self.input_parser.process(&bytes) {
            match input {
                ParsedInput::Raw(bytes) => {
                    self.session_manager_handle.user_input(self.id, bytes).await?;
                }
                ParsedInput::Builtin(action) => {
                    self.execute_builtin_action(action).await?;
                }
                ParsedInput::Named(name) => {
                    self.invoke_named_action(&name).await?;
                }
            }
        }

        Ok(())
    }

    async fn invoke_named_action(&self, name: &str) -> Result<()> {
        for action in self.config_runtime.invoke_named_action(name)? {
            self.execute_builtin_action(action).await?;
        }
        Ok(())
    }

    async fn execute_builtin_action(&self, action: BuiltinAction) -> Result<()> {
        match action {
            BuiltinAction::SplitPaneVertical => {
                self.session_manager_handle
                    .user_split_pane(self.id, SplitDirection::Vertical)
                    .await
            }
            BuiltinAction::SplitPaneHorizontal => {
                self.session_manager_handle
                    .user_split_pane(self.id, SplitDirection::Horizontal)
                    .await
            }
            BuiltinAction::FocusPaneLeft => self.session_manager_handle.user_focus_pane(self.id, FocusDirection::Left).await,
            BuiltinAction::FocusPaneDown => self.session_manager_handle.user_focus_pane(self.id, FocusDirection::Down).await,
            BuiltinAction::FocusPaneUp => self.session_manager_handle.user_focus_pane(self.id, FocusDirection::Up).await,
            BuiltinAction::FocusPaneRight => self.session_manager_handle.user_focus_pane(self.id, FocusDirection::Right).await,
            BuiltinAction::KillPane => self.session_manager_handle.user_kill_pane(self.id).await,
            BuiltinAction::Detach => self.session_manager_handle.client_disconnect(self.id).await,
            BuiltinAction::OpenSessionSwitcher => {
                self.session_manager_handle.client_open_session_switcher(self.id).await
            }
        }
    }
}

use std::{io::Stdout, time::Duration};

use bytes::Bytes;
use crossterm::event::Event;
use ratatui::{Terminal, prelude::CrosstermBackend};
use remux_core::{
    comm,
    events::{CliEvent, DaemonEvent},
    states::DaemonState,
};
use tokio::{net::UnixStream, sync::mpsc, time::interval};
use vt100::Parser;

use crate::{
    actors::ui2,
    input::{self, Input},
    input_parser::{self, InputParser},
    prelude::*,
};

struct Ui {}

pub struct StatusLineState {}

pub enum UiState {
    Normal,
    Selecting,
}

pub struct TerminalState {
    pub emulator: Parser,
    pub size: (u16, u16),
    pub needs_resize: bool,
}

pub struct AppState {
    pub terminal: TerminalState,
    pub daemon: DaemonState,
    pub ui: UiState,
}

pub struct App {
    pub state: AppState,
    input_parser: InputParser,
    ui: Ui,
    stream: UnixStream,
}

impl App {
    pub fn new(stream: UnixStream) -> Self {
        Self {
            stream,
            input_parser: InputParser::default(),
            state: AppState {
                terminal: TerminalState {
                    emulator: Parser::default(),
                    size: (0, 0),
                    needs_resize: true,
                },
                daemon: DaemonState::default(),
                ui: UiState::Normal,
            },
            ui: Ui {},
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        let (tx, mut rx) = mpsc::channel::<Input>(100);
        input::start_input_listener(tx);
        let mut ticker = interval(Duration::from_millis(50));
        let mut term = ratatui::init();
        loop {
            // need an initial render since ui updates app state to convey terminal size information
            term.draw(|f| ui2::draw(f, &mut self.state)).unwrap();
            if self.state.terminal.needs_resize {
                self.state
                    .terminal
                    .emulator
                    .set_size(self.state.terminal.size.0, self.state.terminal.size.1);
                self.state.terminal.needs_resize = false;
            }
            tokio::select! {
                Some(input) = rx.recv() => {
                    use Input::{Stdin, Crossterm};
                    match input {
                        Stdin(bytes) => {
                            self.dispatch_stdin(bytes).await;
                        }
                        Crossterm(event) => {
                            self.dispatch_crossterm(event, &mut term).await;
                        }
                    }
                }
                res = comm::recv_daemon_event(&mut self.stream) => {
                    match res {
                        Ok(event) => {
                            match event {
                                DaemonEvent::Raw(bytes) => {
                                    trace!("DaemonEvent(Raw({bytes:?}))");
                                    self.state.terminal.emulator.process(&bytes);
                                }
                                _ => {
                                    todo!();
                                }
                                // DaemonEvent::Disconnected => {
                                //     debug!("DaemonEvent(Disconnected)");
                                //     self.ui_handle.kill().await.unwrap();
                                //     break;
                                // }
                                // DaemonEvent::CurrentSessions(session_ids) => {
                                //     debug!("DaemonEvent(CurrentSessions({session_ids:?}))");
                                //     self.daemon_state.set_sessions(session_ids);
                                //     self.sync_daemon_state = true;
                                // }
                                // DaemonEvent::ActiveSession(session_id) => {
                                //     debug!("DaemonEvent(ActiveSession({session_id}))");
                                //     self.daemon_state.set_active_session(session_id);
                                //     self.sync_daemon_state = true;
                                // }
                                // DaemonEvent::NewSession(session_id) => {
                                //     debug!("DaemonEvent(NewSession({session_id}))");
                                //     self.daemon_state.add_session(session_id);
                                //     self.sync_daemon_state = true;
                                // }
                                // DaemonEvent::DeletedSession(_session_id) => {
                                //     todo!("implement delete session");
                                // }
                            }
                        }
                        Err(e) => {
                            error!("Error receiving daemon event: {e}");
                            break;
                        }
                    }
                }
                _ = ticker.tick() => {
                    term.draw(|f| ui2::draw(f, &mut self.state)).unwrap();
                }
            }
        }
        Ok(())
    }

    async fn dispatch_stdin(&mut self, bytes: Bytes) {
        for parsed_event in self.input_parser.process(&bytes) {
            match parsed_event {
                input_parser::ParsedEvent::LocalAction(_action) => {
                    todo!("update the application state")
                }
                input_parser::ParsedEvent::DaemonAction(cli_event) => {
                    comm::send_event(&mut self.stream, cli_event).await;
                }
            }
        }
    }

    async fn dispatch_crossterm(&mut self, event: Event, term: &mut Terminal<CrosstermBackend<Stdout>>) {
        match event {
            Event::FocusGained => todo!(),
            Event::FocusLost => todo!(),
            Event::Key(key_event) => match key_event {
                _ => todo!(),
            },
            Event::Mouse(mouse_event) => match mouse_event {
                _ => todo!(),
            },
            Event::Paste(_) => todo!(),
            Event::Resize(_, _) => {
                self.state.terminal.needs_resize = true;
                term.draw(|f| {
                    ui2::draw(f, &mut self.state);
                })
                .unwrap();
                let (rows, cols) = self.state.terminal.size;
                comm::send_event(&mut self.stream, CliEvent::TerminalResize { rows, cols })
                    .await
                    .unwrap();
            }
        }
    }
}

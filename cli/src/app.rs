use std::{
    fmt::Debug,
    io::{Stdout, stdout},
    time::Duration,
};

use bytes::Bytes;
use crossterm::{
    cursor::Show,
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, enable_raw_mode},
};
use derivative::Derivative;
use ratatui::{Terminal, crossterm::terminal::disable_raw_mode, prelude::CrosstermBackend, restore};
use remux_core::{
    comm,
    events::{CliEvent, DaemonEvent},
    states::DaemonState,
};
use tokio::{
    net::UnixStream,
    sync::{mpsc, watch},
    time::interval,
};
use vt100::Parser;

use crate::{
    input::{self, Input},
    input_parser::{self, InputParser},
    prelude::*,
    ui,
};

#[derive(Debug)]
pub struct StatusLineState {}

#[derive(Debug)]
pub enum UiState {
    Normal,
    Selecting,
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct TerminalState {
    #[derivative(Debug = "ignore")]
    pub emulator: Parser,
    pub size: (u16, u16),
    pub needs_resize: bool,
}

#[derive(Debug)]
pub struct AppState {
    pub terminal: TerminalState,
    pub daemon: DaemonState,
    pub ui: UiState,
}

pub struct App {
    pub state: AppState,
    input_parser: InputParser,
    stream: UnixStream,
    bg_tasks: Vec<CliTask>,
}

impl App {
    pub fn new(stream: UnixStream, daemon_state: DaemonState) -> Self {
        Self {
            stream,
            input_parser: InputParser::default(),
            state: AppState {
                terminal: TerminalState {
                    emulator: Parser::default(),
                    size: (0, 0),
                    needs_resize: true,
                },
                daemon: daemon_state,
                ui: UiState::Normal,
            },
            bg_tasks: Vec::new(),
        }
    }

    #[instrument(skip(self))]
    pub async fn run(&mut self) -> Result<()> {
        debug!("starting app");
        let (tx, mut rx) = mpsc::channel::<Input>(100);
        self.bg_tasks.extend(input::start_input_listeners(tx));
        let mut ticker = interval(Duration::from_millis(50));
        let mut term = ratatui::Terminal::new(CrosstermBackend::new(stdout())).unwrap();
        install_panic_hook();
        enable_raw_mode()?;
        debug!("Enabled raw mode");
        execute!(stdout(), EnterAlternateScreen)?;
        debug!("Entered alternate screen");

        // need an initial render since ui updates app state to convey terminal size information
        term.draw(|f| ui::draw(f, &mut self.state)).unwrap();
        loop {
            if self.state.terminal.needs_resize {
                let (rows, cols) = self.state.terminal.size;
                debug!("setting terminal emulator size (rows={rows}, cols={cols})");
                self.state.terminal.emulator.set_size(rows, cols);
                self.state.terminal.needs_resize = false;
                let (rows, cols) = self.state.terminal.size;
                comm::send_event(&mut self.stream, CliEvent::TerminalResize { rows, cols })
                    .await
                    .unwrap();
            }
            tokio::select! {
                Some(input) = rx.recv() => {
                    use Input::{Stdin, Resize};
                    match input {
                        Stdin(bytes) => {
                            trace!("stdin({bytes:?}");
                            self.dispatch_stdin(bytes).await;
                        }
                        Resize => {
                            debug!("resize");
                            self.handle_resize(&mut term).await;
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
                                DaemonEvent::Disconnected => {
                                    debug!("DaemonEvent(Disconnected)");
                                    break;
                                }
                                _ => {
                                    // todo!();
                                }
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
                    term.draw(|f| ui::draw(f, &mut self.state)).unwrap();
                }
            }
        }
        for task in self.bg_tasks.drain(..) {
            task.abort();
            let _ = task.await;
        }
        drop(term);
        restore();
        debug!("Restoring terminal");
        Ok(())
    }

    #[instrument(skip(self, bytes))]
    async fn dispatch_stdin(&mut self, bytes: Bytes) {
        for parsed_event in self.input_parser.process(&bytes) {
            match parsed_event {
                input_parser::ParsedEvent::LocalAction(_action) => {
                    todo!("update the application state")
                }
                input_parser::ParsedEvent::DaemonAction(cli_event) => {
                    debug!("sending cli event: {cli_event:?}");
                    comm::send_event(&mut self.stream, cli_event).await.unwrap();
                }
            }
        }
    }

    #[instrument(skip(self, term))]
    async fn handle_resize(&mut self, term: &mut Terminal<CrosstermBackend<Stdout>>) {
        self.state.terminal.needs_resize = true;
        term.draw(|f| {
            ui::draw(f, &mut self.state);
        })
        .unwrap();
    }
}
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let _ = execute!(stdout(), LeaveAlternateScreen, Show);
        let _ = disable_raw_mode();
        eprintln!("{}", info);
    }));
}

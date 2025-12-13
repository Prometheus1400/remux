use std::{fmt::Debug, io::Stdout, time::Duration};

use bytes::Bytes;
use color_eyre::eyre;
use derivative::Derivative;
use ratatui::{Terminal, prelude::CrosstermBackend, restore, widgets::ListState};
use remux_core::{
    comm,
    events::{CliEvent, DaemonEvent},
    states::DaemonState,
};
use terminput::Event;
use tokio::{
    net::UnixStream,
    sync::{broadcast, mpsc},
    time::interval,
};
use uuid::Uuid;
use vt100::Parser;

use crate::{
    input_parser::{self, InputParser},
    prelude::*,
    states::status_line_state::StatusLineState,
    tasks::{
        input::{self, Input},
        lua,
    },
    ui::{
        self, basic_selector_widget::BasicSelectorWidget, fuzzy_selector_widget::FuzzySelectorWidget,
        traits::SelectorStatefulWidget,
    },
};

#[derive(Derivative)]
#[derivative(Debug)]
pub struct TerminalState {
    #[derivative(Debug = "ignore")]
    pub emulator: Parser,
    pub size: (u16, u16),
    pub needs_resize: bool,
}

#[derive(Debug)]
pub struct UiState {
    pub selector: SelectorState,
    pub status_line: StatusLineState,
}

#[derive(Debug)]
pub enum SelectorType {
    Basic,
    Fuzzy,
}

#[derive(Debug, Clone)]
pub struct IndexedItem {
    pub index: usize,
    pub item: String,
}

impl IndexedItem {
    pub fn new(index: usize, item: String) -> Self {
        Self { index, item }
    }
}

#[derive(Debug)]
pub struct SelectorState {
    pub selector_type: SelectorType,
    pub list_state: ListState,
    pub list: Vec<String>,
    pub query: String,
    // selector might filter out some items - so we need to
    // maintain it's original index to be able to return it
    pub displaying_list: Vec<IndexedItem>,
}

#[derive(Debug)]
pub enum AppMode {
    Normal,
    SelectingSession,
}

#[derive(Debug)]
pub struct AppState {
    pub terminal: TerminalState,
    pub daemon: DaemonState,
    pub ui: UiState,
    pub mode: AppMode,
}

pub struct App {
    pub state: AppState,
    input_parser: InputParser,
    stream: UnixStream,
    bg_tasks: Vec<CliTask>,
    id: Uuid,
}

impl App {
    pub fn new(id: Uuid, stream: UnixStream, daemon_state: DaemonState) -> Self {
        Self {
            id,
            stream,
            input_parser: InputParser::default(),
            state: AppState {
                mode: AppMode::Normal,
                terminal: TerminalState {
                    emulator: Parser::default(),
                    size: (0, 0),
                    needs_resize: true,
                },
                daemon: daemon_state,
                ui: UiState {
                    selector: SelectorState {
                        list_state: ListState::default(),
                        list: Vec::new(),
                        selector_type: SelectorType::Basic,
                        query: String::new(),
                        displaying_list: Vec::new(),
                    },
                    status_line: StatusLineState::default(),
                },
            },
            bg_tasks: Vec::new(),
        }
    }

    #[instrument(parent=None, skip(self), fields(id=?self.id), name="App")]
    pub async fn run(&mut self) -> Result<()> {
        let mut term = ratatui::init();
        debug!("Starting app");
        let (input_tx, mut input_rx) = mpsc::channel::<Input>(100);
        let (lua_tx, mut lua_rx) = broadcast::channel(100);
        self.bg_tasks.extend(input::start_input_listeners(input_tx));
        self.bg_tasks.push(lua::start_status_line_task(lua_tx)?);
        let mut ticker = interval(Duration::from_millis(50));

        // need an initial render since ui updates app state to convey terminal size information
        term.draw(|f| ui::draw(f, &mut self.state))?;
        loop {
            if self.state.terminal.needs_resize {
                let (rows, cols) = self.state.terminal.size;
                info!(rows = rows, cols = cols, "Setting terminal emulator size");
                self.state.terminal.emulator.set_size(rows, cols);
                self.state.terminal.needs_resize = false;
                let (rows, cols) = self.state.terminal.size;
                comm::send_event(&mut self.stream, CliEvent::TerminalResize { rows, cols }).await?;
            }
            tokio::select! {
                Some(input) = input_rx.recv() => {
                    let span = error_span!("Recieved Input");
                    let _guard = span.enter();
                    use Input::{Stdin, Resize};
                    match &input {
                        Stdin(bytes) => {
                            trace!(input=?input);
                            self.dispatch_stdin(bytes.clone()).await.unwrap();
                        }
                        Resize => {
                            info!(input=?input);
                            self.handle_resize(&mut term).await.unwrap();
                        }
                    }
                }
                Ok(mut status_line_state) = lua_rx.recv() => {
                    trace!(status_line_state=?status_line_state, "received status line state");
                    status_line_state.apply_built_ins(&self.state);
                    self.state.ui.status_line = status_line_state;
                }
                res = comm::recv_daemon_event(&mut self.stream) => {
                    match res {
                        Ok(event) => {
                            let span = error_span!("Recieved Daemon Event");
                            let _guard = span.enter();
                            match &event {
                                DaemonEvent::Raw(bytes) => {
                                    trace!(event=?event, num_bytes=bytes.len());
                                }
                                _ => {
                                    info!(event=?event);
                                }
                            }
                            match event {
                                DaemonEvent::Raw(bytes) => {
                                    self.state.terminal.emulator.process(&bytes);
                                }
                                DaemonEvent::Disconnected => {
                                    break;
                                }
                                DaemonEvent::ActiveSession(session_id) => {
                                    self.state.daemon.set_active_session(session_id);
                                }
                                DaemonEvent::NewSession(session_id, session_name) => {
                                    self.state.daemon.add_session(session_id, session_name);
                                }
                                _ => {
                                    todo!();
                                }
                            }
                        }
                        Err(e) => {
                            error!(error=%e, "Error receiving daemon event");
                            // break;
                        }
                    }
                }
                _ = ticker.tick() => {
                    term.draw(|f| ui::draw(f, &mut self.state))?;
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

    async fn dispatch_stdin(&mut self, bytes: Bytes) -> Result<()> {
        match self.state.mode {
            AppMode::Normal => self.handle_stdin_for_normal_mode(bytes).await?,
            AppMode::SelectingSession => self.handle_stdin_for_selecting_mode(bytes).await?,
        }

        Ok(())
    }

    async fn handle_stdin_for_selecting_mode(&mut self, bytes: Bytes) -> Result<()> {
        let event = Event::parse_from(&bytes)?.unwrap();

        let selection_opt = match self.state.ui.selector.selector_type {
            SelectorType::Basic => BasicSelectorWidget::input(event, &mut self.state.ui.selector),
            SelectorType::Fuzzy => FuzzySelectorWidget::input(event, &mut self.state.ui.selector),
        };
        if let Some(selection) = selection_opt {
            match selection {
                ui::traits::Selection::Index(i) => match self.state.mode {
                    AppMode::SelectingSession => {
                        let session = &self.state.daemon.sessions[i];
                        comm::send_event(&mut self.stream, CliEvent::SwitchSession(session.id)).await?;
                    }
                    AppMode::Normal => {}
                },
                ui::traits::Selection::Cancelled => {}
            }
            self.state.mode = AppMode::Normal;
            self.state.ui.selector.list_state.select(Some(0));
            self.state.ui.selector.list.clear();
        }
        Ok(())
    }

    async fn handle_stdin_for_normal_mode(&mut self, bytes: Bytes) -> Result<()> {
        for parsed_event in self.input_parser.process(&bytes) {
            match parsed_event {
                input_parser::ParsedEvent::LocalAction(action) => {
                    self.dispatch_action(action).await;
                }
                input_parser::ParsedEvent::DaemonAction(cli_event) => {
                    comm::send_event(&mut self.stream, cli_event).await?;
                }
            }
        }
        Ok(())
    }

    async fn dispatch_action(&mut self, action: input_parser::Action) {
        match action {
            input_parser::Action::SwitchSession => {
                self.state.mode = AppMode::SelectingSession;
                self.state.ui.selector.query.clear();
                self.state.ui.selector.list_state.select(Some(0));
                self.state.ui.selector.selector_type = SelectorType::Basic;
                self.state
                    .ui
                    .selector
                    .list
                    .extend(self.state.daemon.sessions.iter().map(|x| x.name.clone()));
                self.state.ui.selector.displaying_list = self
                    .state
                    .ui
                    .selector
                    .list
                    .iter()
                    .enumerate()
                    .map(|(i, x)| IndexedItem::new(i, x.clone()))
                    .collect();
            }
        }
    }

    #[instrument(skip(self, term))]
    async fn handle_resize(&mut self, term: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
        self.state.terminal.needs_resize = true;
        term.draw(|f| {
            ui::draw(f, &mut self.state);
        })?;

        eyre::Ok(())
    }
}

use std::{fmt::Debug, io::Stdout, time::Duration};

use bytes::Bytes;
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
}

impl App {
    pub fn new(stream: UnixStream, daemon_state: DaemonState) -> Self {
        Self {
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

    #[instrument(skip(self))]
    pub async fn run(&mut self) -> Result<()> {
        let mut term = ratatui::init();
        debug!("starting app");
        let (input_tx, mut input_rx) = mpsc::channel::<Input>(100);
        let (lua_tx, mut lua_rx) = broadcast::channel(100);
        self.bg_tasks.extend(input::start_input_listeners(input_tx));
        self.bg_tasks.push(lua::start_status_line_task(lua_tx));
        let mut ticker = interval(Duration::from_millis(50));
        debug!("Enabled raw mode");
        // execute!(stdout(), EnterAlternateScreen)?;
        debug!("Entered alternate screen");

        // need an initial render since ui updates app state to convey terminal size information
        term.draw(|f| ui::draw(f, &mut self.state))?;
        loop {
            if self.state.terminal.needs_resize {
                let (rows, cols) = self.state.terminal.size;
                debug!("setting terminal emulator size (rows={rows}, cols={cols})");
                self.state.terminal.emulator.set_size(rows, cols);
                self.state.terminal.needs_resize = false;
                let (rows, cols) = self.state.terminal.size;
                comm::send_event(&mut self.stream, CliEvent::TerminalResize { rows, cols }).await?;
            }
            tokio::select! {
                Some(input) = input_rx.recv() => {
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
                Ok(mut status_line_state) = lua_rx.recv() => {
                    debug!("received status line state");
                    status_line_state.apply_built_ins(&self.state);
                    self.state.ui.status_line = status_line_state;
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
                                DaemonEvent::ActiveSession(session_id) => {
                                    debug!("DaemonEvent(ActiveSession({session_id}))");
                                    self.state.daemon.set_active_session(session_id);
                                }
                                DaemonEvent::NewSession(session_id) => {
                                    debug!("DaemonEvent(NewSession({session_id}))");
                                    self.state.daemon.add_session(session_id);
                                }
                                _ => {
                                    todo!();
                                }
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

    #[instrument(skip(self, bytes))]
    async fn dispatch_stdin(&mut self, bytes: Bytes) -> Result<()> {
        match self.state.mode {
            AppMode::Normal => self.handle_stdin_for_normal_mode(bytes).await?,
            AppMode::SelectingSession => self.handle_stdin_for_selecting_mode(bytes).await?,
        }

        Ok(())
    }

    async fn handle_stdin_for_selecting_mode(&mut self, bytes: Bytes) -> Result<()> {
        let event = Event::parse_from(&bytes)?.unwrap();
        panic!("hi");

        let selection_opt = match self.state.ui.selector.selector_type {
            SelectorType::Basic => BasicSelectorWidget::input(event, &mut self.state.ui.selector),
            SelectorType::Fuzzy => FuzzySelectorWidget::input(event, &mut self.state.ui.selector),
        };
        if let Some(selection) = selection_opt {
            match selection {
                ui::traits::Selection::Index(i) => match self.state.mode {
                    AppMode::SelectingSession => {
                        let session = self.state.daemon.session_ids[i];
                        comm::send_event(&mut self.stream, CliEvent::SwitchSession(session)).await?;
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
                    debug!("sending cli event: {cli_event:?}");
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
                self.state.ui.selector.selector_type = SelectorType::Fuzzy;
                self.state
                    .ui
                    .selector
                    .list
                    .extend(self.state.daemon.session_ids.iter().map(|x| x.to_string()));
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
    async fn handle_resize(&mut self, term: &mut Terminal<CrosstermBackend<Stdout>>) {
        self.state.terminal.needs_resize = true;
        term.draw(|f| {
            ui::draw(f, &mut self.state);
        })
        .unwrap();
    }
}

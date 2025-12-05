use std::{
    io::stdout,
    sync::{Arc, RwLock},
    time::Duration,
};

use bytes::Bytes;
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use handle_macro::Handle;
use ratatui::{Terminal, prelude::CrosstermBackend};
use remux_core::states::DaemonState;
use tokio::{
    sync::{broadcast, mpsc},
    time::interval,
};
use tracing::Instrument;
use tui_term::widget::PseudoTerminal;
use vt100::Parser;

use crate::{
    prelude::*,
    states::status_line_state::StatusLineState,
    utils::DisplayableVec,
    widgets::{BasicSelector, FuzzySelector, Selector, TerminalWidget},
};
use crate::actors::{
        client::ClientHandle, lua::{Lua, LuaHandle}
    };

#[derive(Handle)]
pub enum UIEvent {
    Output(Bytes),
    Stdin(Bytes),
    Kill,
    SyncDaemonState(DaemonState),
    SyncStatusLineState(StatusLineState),
    SelectBasic { items: DisplayableVec, title: String },
    SelectFuzzy { items: DisplayableVec, title: String },
    SelectedIndex(Option<usize>),
}
use UIEvent::*;

#[derive(Debug, PartialEq)]
enum UIState {
    Normal,
    SelectingBasic,
    SelectingFuzzy,
}

pub struct UI {
    // for communication
    _handle: UIHandle,
    rx: mpsc::Receiver<UIEvent>,
    // state for rendering
    daemon_state: DaemonState,
    status_line_state: StatusLineState,
    parser: Parser,
    lua_handle: LuaHandle,
    client_handle: ClientHandle,
    ui_state: UIState,
    basic_selector: Arc<RwLock<BasicSelector>>,
    fuzzy_selector: Arc<RwLock<FuzzySelector>>,
    selector_rx: mpsc::Receiver<Option<usize>>,
    stdin_rx: mpsc::Receiver<Bytes>,

    terminal_widget: TerminalWidget
}

impl UI {
    #[instrument(skip())]
    pub fn new() -> Result<Self> {
        let (tx, rx) = mpsc::channel(100);
        let _handle = UIHandle { tx };

        let client_handle = Client::spawn(, daemon_state);

        let parser = vt100::Parser::default();
        let lua_handle = Lua::spawn(handle.clone())?;
        let (selector_tx, selector_rx) = mpsc::channel(100);

        Ok(Self {
            _handle,
            rx,

            daemon_state: DaemonState::default(),
            status_line_state: StatusLineState::default(),
            parser,
            lua_handle,
            client_handle,
            ui_state: UIState::Normal,
            basic_selector: BasicSelector::new(selector_tx.clone()),
            fuzzy_selector: FuzzySelector::new(selector_tx.clone()),
            selector_rx,
            stdin_rx,
            terminal_widget: TerminalWidget::default(),
        })
    }
    #[instrument(skip(self), fields(ui_state = ?self.ui_state))]
    pub async fn run(mut self) -> Result<()> {
        let span = tracing::Span::current();
        let mut term = Terminal::new(CrosstermBackend::new(stdout())).unwrap();
        let (selector_tx, _) = broadcast::channel(10000);
        let selector_tx = selector_tx.clone();
        let mut ticker = interval(Duration::from_millis(16));
        execute!(stdout(), EnterAlternateScreen)?;
        term.clear()?;
        let task = tokio::spawn({
            async move {
                loop {
                    tokio::select! {
                        Some(event) = self.rx.recv() => {
                            match event {
                                Output(bytes) => {
                                    self.parser.process(&bytes);
                                }
                                Stdin(bytes) if matches!(self.ui_state, UIState::SelectingFuzzy | UIState::SelectingBasic) => {
                                    selector_tx.send(bytes).unwrap();
                                }
                                Stdin(_) => {
                                    error!("Shouldn't recieve stdin in current state");
                                }
                                SyncDaemonState(daemon_state) => {
                                    self.lua_handle.sync_daemon_state(daemon_state.clone()).unwrap();
                                    self.daemon_state = daemon_state;
                                }
                                SyncStatusLineState(status_line_state) => {
                                    self.status_line_state = status_line_state;
                                }
                                SelectBasic{items, title} => {
                                    self.ui_state = UIState::SelectingBasic;
                                    BasicSelector::run(&self.basic_selector, selector_tx.subscribe(), items, title).unwrap();
                                }
                                SelectFuzzy{items, title} => {
                                    self.ui_state = UIState::SelectingFuzzy;
                                    FuzzySelector::run(&self.fuzzy_selector, selector_tx.subscribe(), items, title).unwrap();
                                }
                                SelectedIndex(index) => {
                                    self.client_handle.selected(index).await.unwrap();
                                    self.ui_state = UIState::Normal;
                                }
                                Kill => {
                                    self.lua_handle.kill().unwrap();
                                    break;
                                }
                            }
                        }
                        _ = ticker.tick() => {
                            let screen = self.parser.screen();
                            term.draw(|f| {
                                use ratatui::{
                                    layout::Constraint, layout::Direction, layout::Layout,
                                };

                                let chunks = Layout::default()
                                    .direction(Direction::Vertical)
                                    .constraints([
                                        Constraint::Min(1),      // pseudo terminal takes everything else
                                        Constraint::Length(1),   // bottom status bar
                                    ])
                                    .split(f.area());

                                // render the normal terminal output
                                let term_ui = PseudoTerminal::new(screen);
                                f.render_widget(term_ui, chunks[0]);

                                // render the status bar
                                self.status_line_state.render(f, chunks[1]);

                                // render selector if active
                                if self.ui_state == UIState::SelectingBasic {
                                    BasicSelector::render(&self.basic_selector, f);
                                }
                                if self.ui_state == UIState::SelectingFuzzy {
                                    FuzzySelector::render(&self.fuzzy_selector, f);
                                }

                            }).unwrap();
                        }
                    }
                }
            debug!("ui stopped");
            execute!(stdout(), LeaveAlternateScreen)?;
            Ok::<(), Error>(())
            }.instrument(span)
        }).await;
        Ok(())
    }
}

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
use tokio::{
    sync::{broadcast, mpsc},
    time::interval,
};
use tracing::Instrument;
use tui_term::widget::PseudoTerminal;
use vt100::Parser;

use crate::{
    actors::{
        client::ClientHandle,
        lua::{Lua, LuaHandle},
    },
    prelude::*,
    states::{daemon_state::DaemonState, status_line_state::StatusLineState},
    utils::DisplayableVec,
    widgets::Selector,
};

#[derive(Handle)]
pub enum UIEvent {
    Output(Bytes),
    Kill,
    SyncDaemonState(DaemonState),
    SyncStatusLineState(StatusLineState),
    Select { items: DisplayableVec, title: String },
}
use UIEvent::*;

#[derive(Debug, PartialEq)]
enum UIState {
    Normal,
    Selecting,
}

pub struct UI {
    // for communication
    handle: UIHandle,
    rx: mpsc::Receiver<UIEvent>,
    // state for rendering
    daemon_state: DaemonState,
    status_line_state: StatusLineState,
    parser: Parser,
    lua_handle: LuaHandle,
    client_handle: ClientHandle,
    ui_state: UIState,
    selector: Arc<RwLock<Selector>>,
    selector_rx: mpsc::Receiver<Option<usize>>,
    stdin_rx: mpsc::Receiver<Bytes>,
}

impl UI {
    #[instrument(skip(client_handle, stdin_rx))]
    pub fn spawn(client_handle: ClientHandle, stdin_rx: mpsc::Receiver<Bytes>) -> Result<UIHandle> {
        Self::new(client_handle, stdin_rx)?.run()
    }
    #[instrument(skip(client_handle, stdin_rx))]
    fn new(client_handle: ClientHandle, stdin_rx: mpsc::Receiver<Bytes>) -> Result<Self> {
        let (tx, rx) = mpsc::channel(100);
        let handle = UIHandle { tx };
        let parser = vt100::Parser::default();
        let lua_handle = Lua::spawn(handle.clone())?;
        let (selector_tx, selector_rx) = mpsc::channel(100);
        Ok(Self {
            daemon_state: DaemonState::default(),
            status_line_state: StatusLineState::default(),
            rx,
            handle,
            parser,
            lua_handle,
            client_handle,
            ui_state: UIState::Normal,
            selector: Selector::new(selector_tx),
            selector_rx,
            stdin_rx,
        })
    }
    #[instrument(skip(self), fields(ui_state = ?self.ui_state))]
    pub fn run(mut self) -> Result<UIHandle> {
        let span = tracing::Span::current();

        let handle_clone = self.handle.clone();
        let mut term = Terminal::new(CrosstermBackend::new(stdout())).unwrap();
        let (selector_tx, _) = broadcast::channel(10000);
        tokio::spawn({
            let selector_tx = selector_tx.clone();
            async move {
                let mut ticker = interval(Duration::from_millis(16));
                execute!(stdout(), EnterAlternateScreen)?;
                term.clear()?;
                loop {
                    tokio::select! {
                        Some(event) = self.rx.recv() => {
                            match event {
                                Output(bytes) => {
                                    self.parser.process(&bytes);
                                }
                                SyncDaemonState(daemon_state) => {
                                    self.lua_handle.sync_daemon_state(daemon_state.clone()).unwrap();
                                    self.daemon_state = daemon_state;
                                }
                                SyncStatusLineState(status_line_state) => {
                                    self.status_line_state = status_line_state;
                                }
                                Select{items, title} => {
                                    self.ui_state = UIState::Selecting;
                                    Selector::run(&self.selector, selector_tx.subscribe(), items, title).unwrap();
                                }
                                Kill => {
                                    self.lua_handle.kill().unwrap();
                                    break;
                                }
                            }
                        }
                        Some(bytes) = self.stdin_rx.recv(), if matches!(self.ui_state, UIState::Selecting) => {
                            selector_tx.send(bytes).unwrap();
                        }
                        Some(index) = self.selector_rx.recv() => {
                            debug!("selected index({index:?})");
                            self.client_handle.selected(index).await.unwrap();
                            self.ui_state = UIState::Normal;
                        }
                        _ = ticker.tick() => {
                            let screen = self.parser.screen();
                            term.draw(|f| {
                                let chunks = ratatui::layout::Layout::default()
                                    .direction(ratatui::layout::Direction::Vertical)
                                    .constraints([
                                        ratatui::layout::Constraint::Min(1),      // pseudo terminal takes everything else
                                        ratatui::layout::Constraint::Length(1),   // bottom status bar
                                    ])
                                    .split(f.area());

                                // render the normal terminal output
                                let term_ui = PseudoTerminal::new(screen);
                                f.render_widget(term_ui, chunks[0]);

                                // render the status bar
                                self.status_line_state.render(f, chunks[1]);

                                // render selector if active
                                if self.ui_state == UIState::Selecting {
                                    Selector::render(&self.selector, f);
                                }

                            }).unwrap();
                        }
                    }
                }
                debug!("ui stopped");
                execute!(stdout(), LeaveAlternateScreen)?;
                Ok::<(), Error>(())
            }
            .instrument(span)
        });

        Ok(handle_clone)
    }
}

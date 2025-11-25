use std::{io::stdout, time::Duration};

use bytes::Bytes;
use handle_macro::Handle;
use ratatui::{
    Terminal,
    prelude::CrosstermBackend,
    widgets::{Block, Paragraph},
};
use tokio::{sync::mpsc, time::interval};
use tui_term::widget::PseudoTerminal;
use vt100::Parser;

use crate::{
    actors::lua::{Lua, LuaHandle},
    prelude::*,
    states::{daemon_state::DaemonState, status_line_state::StatusLineState},
};

#[derive(Handle)]
pub enum UIEvent {
    Output(Bytes),
    ClearTerminal,
    Kill,
    SyncDaemonState(DaemonState),
    SyncStatusLineState(StatusLineState),
}
use UIEvent::*;

pub struct Popup<'a> {
    paragraph: Paragraph<'a>,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
}

pub struct UI<'a> {
    // for communication
    handle: UIHandle,
    rx: mpsc::Receiver<UIEvent>,
    // state for rendering
    daemon_state: DaemonState,
    status_line_state: StatusLineState,
    parser: Parser,
    popups: Vec<Paragraph<'a>>,
    lua_handle: LuaHandle,
}

impl<'a> UI<'a> {
    pub fn spawn() -> Result<UIHandle> {
        Self::new()?.run()
    }
    fn new() -> Result<Self> {
        let (tx, rx) = mpsc::channel(100);
        let handle = UIHandle { tx };
        let parser = vt100::Parser::default();
        let lua_handle = Lua::spawn(handle.clone())?;
        Ok(Self {
            daemon_state: DaemonState::default(),
            status_line_state: StatusLineState::default(),
            rx,
            handle,
            parser,
            popups: vec![],
            lua_handle,
        })
    }
    pub fn run(mut self) -> Result<UIHandle> {
        let handle_clone = self.handle.clone();

        let mut term = Terminal::new(CrosstermBackend::new(stdout())).unwrap();
        term.clear()?;

        let _: CliTask = tokio::spawn({
            async move {
                let mut ticker = interval(Duration::from_millis(16));
                loop {
                    tokio::select! {
                        Some(event) = self.rx.recv() => {
                            match event {
                                Output(bytes) => {
                                    self.parser.process(&bytes);
                                }
                                ClearTerminal => {
                                    self.parser.process(b"\x1b[H\x1b[2J");
                                }
                                SyncDaemonState(daemon_state) => {
                                    self.lua_handle.sync_daemon_state(daemon_state.clone()).unwrap();
                                    self.daemon_state = daemon_state;
                                }
                                SyncStatusLineState(status_line_state) => {
                                    self.status_line_state = status_line_state;
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

                                // 1. Create the blocks
                                let chunks = ratatui::layout::Layout::default()
                                    .direction(ratatui::layout::Direction::Vertical)
                                    .constraints([
                                        ratatui::layout::Constraint::Min(1),      // pseudo terminal takes everything else
                                        ratatui::layout::Constraint::Length(1),   // bottom status bar
                                    ])
                                    .split(f.area());

                                // 2. Render pseudo terminal in the upper chunk
                                let term_ui = PseudoTerminal::new(screen);
                                f.render_widget(term_ui, chunks[0]);

                                // 3. Render a simple 1-line status bar
                                self.status_line_state.render(f, chunks[1]);
                                // if let Some(active_session_id) = self.daemon_state.active_session {
                                //     // let bar = Paragraph::new(format!("current session: {}, all sessions: {:?}", active_session_id, self.daemon_state.session_ids))
                                //     let bar = Paragraph::from(&self.status_line_state)
                                //         .style(ratatui::style::Style::default().bg(ratatui::style::Color::Black));
                                //         f.render_widget(bar, chunks[1]);
                                // }
                            })?;
                        }
                    }
                }
                debug!("ui stopped");
                Ok(())
            }
        });

        Ok(handle_clone)
    }
}

use std::{io::stdout, time::Duration};

use bytes::Bytes;
use handle_macro::Handle;
use ratatui::{
    Terminal,
    prelude::CrosstermBackend,
};
use tokio::{sync::mpsc, time::interval};
use tui_term::widget::PseudoTerminal;
use vt100::Parser;

use crate::prelude::*;

#[derive(Handle)]
pub enum UIEvent {
    Output(Bytes),
    Kill,
    // SyncStateView(StateView),
}
use UIEvent::*;

pub struct UI {
    // state_view: StateView,
    parser: Parser,
    handle: UIHandle,
    rx: mpsc::Receiver<UIEvent>,
}

impl UI {
    pub fn spawn() -> Result<UIHandle> {
        Self::new().run()
    }
    fn new() -> Self {
        let (tx, rx) = mpsc::channel(100);
        let handle = UIHandle { tx };
        let parser = vt100::Parser::default();
        Self { rx, handle, parser }
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
                                Kill => {
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
                                let bar = ratatui::widgets::Paragraph::new("status bar")
                                    .style(ratatui::style::Style::default().bg(ratatui::style::Color::Black));
                                f.render_widget(bar, chunks[1]);
                            })?;
                        }
                    }
                }
                Ok(())
            }
        });

        Ok(handle_clone)
    }
}

use std::io::{Stdout, stdout};

use bytes::Bytes;
use crossterm::{
    ExecutableCommand,
    cursor::{Hide, Show},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use handle_macro::Handle;
use ratatui::{Terminal, prelude::CrosstermBackend};
use tokio::sync::mpsc;
use tracing::Instrument;

use crate::{
    actors::client::ClientHandle,
    prelude::*,
    widgets::{AlternateScreenWidget, SelectorWidget},
};

#[derive(Handle)]
pub enum WidgetRunnerEvent {
    SelectSession { items: Vec<u32> },
}

#[derive(Debug)]
pub struct WidgetRunner {
    stdin_rx: mpsc::Receiver<Bytes>,
    handle: WidgetRunnerHandle,
    rx: mpsc::Receiver<WidgetRunnerEvent>,
    term: Terminal<CrosstermBackend<Stdout>>,
    client_handle: ClientHandle,
}

impl WidgetRunner {
    pub fn spawn(
        stdin_rx: mpsc::Receiver<Bytes>,
        client_handle: ClientHandle,
    ) -> Result<WidgetRunnerHandle> {
        Self::new(stdin_rx, client_handle)?.run()
    }

    fn new(stdin_rx: mpsc::Receiver<Bytes>, client_handle: ClientHandle) -> Result<Self> {
        let (tx, rx) = mpsc::channel(100);
        let handle = WidgetRunnerHandle { tx };
        let term = Terminal::new(CrosstermBackend::new(stdout()))?;
        Ok(Self {
            stdin_rx,
            handle,
            rx,
            term,
            client_handle,
        })
    }

    #[instrument(skip(self))]
    fn run(mut self) -> Result<WidgetRunnerHandle> {
        let span = tracing::Span::current();
        let handle_clone = self.handle.clone();

        let _: CliTask = tokio::spawn({
            async move {
                loop {
                    if let Some(event) = self.rx.recv().await {
                        match event {
                            WidgetRunnerEvent::SelectSession { items } => {
                                let viewable_items = items.iter().map(|i| i.to_string()).collect();
                                if let Some(index) = self
                                    .run_in_alt_context(SelectorWidget::new(
                                        viewable_items,
                                        "Select Session",
                                    ))
                                    .await
                                {
                                    let session_id = items[index];
                                    self.client_handle.switch_session(Some(session_id)).await?;
                                } else {
                                    self.client_handle.switch_session(None).await?;
                                }
                            }
                        }
                    }
                }
            }
            .instrument(span)
        });

        Ok(handle_clone)
    }

    async fn run_in_alt_context<F, T>(&mut self, widget: F) -> Option<T>
    where
        F: AlternateScreenWidget<T>,
    {
        self.term.backend_mut().execute(EnterAlternateScreen).ok()?;
        self.term.backend_mut().execute(Hide).ok()?;
        self.term.clear();
        let res = widget.run(&mut self.term, &mut self.stdin_rx).await;
        self.term.backend_mut().execute(LeaveAlternateScreen).ok()?;
        self.term.backend_mut().execute(Show).ok()?;
        res
    }
}

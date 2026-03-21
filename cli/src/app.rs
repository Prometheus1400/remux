use std::io::{self, Write};

use bytes::Bytes;
use crossterm::{
    cursor::{Hide, Show},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use remux_core::{
    comm,
    events::{CliEvent, DaemonEvent},
};
use tokio::{net::UnixStream, sync::mpsc};
use uuid::Uuid;

use crate::{
    input_parser::{self, InputParser},
    prelude::*,
    tasks::input::{self, Input},
};

pub struct App {
    _id: Uuid,
    input_parser: InputParser,
    stream: UnixStream,
    bg_tasks: Vec<CliTask>,
    terminal_size: (u16, u16),
}

impl App {
    pub fn new(id: Uuid, stream: UnixStream) -> Self {
        Self {
            _id: id,
            input_parser: InputParser::default(),
            stream,
            bg_tasks: Vec::new(),
            terminal_size: (0, 0),
        }
    }

    #[instrument(parent=None, skip(self), name="App")]
    pub async fn run(&mut self) -> Result<()> {
        self.enter_terminal()?;

        let result = self.run_loop().await;

        for task in self.bg_tasks.drain(..) {
            task.abort();
            let _ = task.await;
        }

        self.restore_terminal()?;
        result
    }

    async fn run_loop(&mut self) -> Result<()> {
        let (input_tx, mut input_rx) = mpsc::channel::<Input>(100);
        self.bg_tasks.extend(input::start_input_listeners(input_tx));
        self.capture_terminal_size()?;
        self.send_terminal_resize().await?;

        loop {
            tokio::select! {
                Some(input) = input_rx.recv() => {
                    match input {
                        Input::Stdin(bytes) => self.dispatch_stdin(bytes).await?,
                        Input::Resize => {
                            self.capture_terminal_size()?;
                            self.send_terminal_resize().await?;
                        }
                    }
                }
                event = comm::recv_daemon_event(&mut self.stream) => {
                    match event? {
                        DaemonEvent::Raw(bytes) => self.write_output(bytes)?,
                        DaemonEvent::Disconnected => break,
                    }
                }
            }
        }

        Ok(())
    }

    async fn dispatch_stdin(&mut self, bytes: Bytes) -> Result<()> {
        for parsed_event in self.input_parser.process(&bytes) {
            match parsed_event {
                input_parser::ParsedEvent::DaemonAction(cli_event) => {
                    comm::send_event(&mut self.stream, cli_event).await?;
                }
            }
        }

        Ok(())
    }
    fn enter_terminal(&self) -> Result<()> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, Hide)?;
        Ok(())
    }

    fn restore_terminal(&self) -> Result<()> {
        execute!(io::stdout(), Show, LeaveAlternateScreen)?;
        disable_raw_mode()?;
        Ok(())
    }

    fn capture_terminal_size(&mut self) -> Result<()> {
        let (cols, rows) = terminal::size()?;
        self.terminal_size = (rows, cols);
        Ok(())
    }

    async fn send_terminal_resize(&mut self) -> Result<()> {
        let (rows, cols) = self.terminal_size;
        if rows == 0 || cols == 0 {
            return Ok(());
        }
        comm::send_event(&mut self.stream, CliEvent::TerminalResize { rows, cols })
            .await
            .map_err(Into::into)
    }

    fn write_output(&mut self, bytes: Bytes) -> Result<()> {
        let mut stdout = io::stdout();
        stdout.write_all(&bytes)?;
        stdout.flush()?;
        Ok(())
    }
}

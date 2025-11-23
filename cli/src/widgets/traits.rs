use std::io::Stdout;

use bytes::Bytes;
use ratatui::{Terminal, prelude::CrosstermBackend};
use tokio::sync::mpsc;

pub trait AlternateScreenWidget<T> {
    async fn run(
        &self,
        term: &mut Terminal<CrosstermBackend<Stdout>>,
        rx: &mut mpsc::Receiver<Bytes>,
    ) -> Option<T>;
}

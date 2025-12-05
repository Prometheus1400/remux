use std::sync::{Arc, RwLock};

use bytes::Bytes;
use crossterm::event::Event;
use ratatui::{Frame, layout::Rect};
use tokio::sync::broadcast;

use crate::{prelude::*, utils::DisplayableVec};

pub trait Selector {
    fn run<T: Into<String>>(
        selector: &Arc<RwLock<Self>>,
        rx: broadcast::Receiver<Bytes>,
        items: DisplayableVec,
        title: T,
    ) -> Result<()>;

    fn render(selector: &Arc<RwLock<Self>>, f: &mut Frame);
}

pub trait Component {
    fn handle_event(&mut self, event: Option<Event>);

    fn render(&mut self, f: &mut Frame, rect: Rect);
}

use std::{
    fmt::Display,
    io::{Stdout, stdout},
    time::Duration,
};

use bytes::Bytes;
use crossterm::{
    ExecutableCommand,
    cursor::{Hide, Show},
    event::{self},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    prelude::CrosstermBackend,
    style::{Style, Stylize},
    widgets::{List, ListItem, ListState},
};
use terminput::Event;
use tokio::sync::mpsc;
use tracing::debug;

use crate::prelude::*;

pub async fn selector_widget<V>(
    term: &mut Terminal<CrosstermBackend<Stdout>>,
    rx: &mut mpsc::Receiver<Bytes>,
    items: &[V],
) -> Option<usize>
where
    V: Into<String> + Display + Clone,
{
    term.backend_mut().execute(EnterAlternateScreen).unwrap();
    term.backend_mut().execute(Hide).unwrap();
    // enable_raw_mode().unwrap();
    term.clear();
    let list_items: Vec<ListItem> = items.iter().map(|i| ListItem::new(i.to_string())).collect();
    let mut state = ListState::default().with_selected(Some(0));
    loop {
        term.draw(|f| {
            debug!("rerendering widget");
            let list = List::new(list_items.clone())
                .highlight_symbol(">> ")
                .highlight_style(Style::default().bold());
            f.render_stateful_widget(list, f.area(), &mut state);
        })
        .ok()?;
        if let Some(bytes) = rx.recv().await {
            debug!("received input in widget");
            if let Event::Key(key_event) = Event::parse_from(&bytes).unwrap().unwrap() {
                use terminput::KeyCode::*;
                match key_event.code {
                    Up | Char('k') => {
                        let i = match state.selected() {
                            Some(i) if i > 0 => i - 1,
                            _ => 0,
                        };
                        state.select(Some(i));
                    }
                    Down | Char('j') => {
                        let i = match state.selected() {
                            Some(i) if i < items.len() - 1 => i + 1,
                            _ => items.len() - 1,
                        };
                        state.select(Some(i));
                    }
                    Enter => {
                        // disable_raw_mode().unwrap();
                        term.backend_mut().execute(LeaveAlternateScreen).unwrap();
                        return state.selected();
                    }
                    Esc | Char('q') => {
                        debug!("selector popup: esc or q");
                        // disable_raw_mode().unwrap();
                        term.backend_mut().execute(LeaveAlternateScreen).unwrap();
                        term.backend_mut().execute(Show).unwrap();
                        return None;
                    }
                    _ => {}
                }
            }
        }
    }
}

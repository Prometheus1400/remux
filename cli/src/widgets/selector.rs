use std::{
    fmt::Display,
    io::{Stdout, stdout},
    time::Duration,
};

use crossterm::{
    ExecutableCommand, cursor::{Hide, Show}, event::{self, Event, KeyCode}, execute, terminal::{EnterAlternateScreen, LeaveAlternateScreen, enable_raw_mode}
};
use ratatui::{
    Terminal, crossterm::terminal::disable_raw_mode, prelude::CrosstermBackend, style::{Style, Stylize}, widgets::{List, ListItem, ListState}
};
use serde::de;
use tracing::debug;

use crate::prelude::*;

pub async fn selector_widget<V>(
    term: &mut Terminal<CrosstermBackend<Stdout>>,
    items: &[V],
) -> Option<usize>
where
    V: Into<String> + Display + Clone,
{
    term.backend_mut().execute(EnterAlternateScreen).unwrap();
    term.backend_mut().execute(Hide).unwrap();
    enable_raw_mode().unwrap();
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
        if !event::poll(Duration::from_millis(100)).ok()? {
            continue;
        }
        if let Event::Key(key) = event::read().ok()? {
            debug!("detected key press");
            use KeyCode::*;
            match key.code {
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
                    // execute!(stdout(), LeaveAlternateScreen).unwrap();
                    disable_raw_mode().unwrap();
                    term.backend_mut().execute(LeaveAlternateScreen).unwrap();
                    return state.selected();
                }
                Esc | Char('q') => {
                    debug!("selector popup: esc or q");
                    // execute!(stdout(), LeaveAlternateScreen).unwrap();
                    disable_raw_mode().unwrap();
                    term.backend_mut().execute(LeaveAlternateScreen).unwrap();
                    term.backend_mut().execute(Show).unwrap();
                    return None;
                }
                _ => {}
            }
        }
    }
}

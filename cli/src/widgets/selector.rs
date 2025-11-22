use std::{fmt::Display, io::Stdout};

use bytes::Bytes;
use crossterm::{
    ExecutableCommand,
    cursor::{Hide, Show},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    Terminal,
    layout::Rect,
    prelude::CrosstermBackend,
    style::{Modifier, Style, Stylize},
    widgets::{Block, Borders, List, ListItem, ListState},
};
use terminput::Event;
use tokio::sync::mpsc;

use crate::prelude::*;

pub async fn selector_widget<V>(
    term: &mut Terminal<CrosstermBackend<Stdout>>,
    rx: &mut mpsc::Receiver<Bytes>,
    items: &[V],
    title: &str,
) -> Option<usize>
where
    V: Into<String> + Display + Clone,
{
    term.backend_mut().execute(EnterAlternateScreen).ok()?;
    term.backend_mut().execute(Hide).ok()?;
    term.clear();
    let list_items: Vec<ListItem> = items.iter().map(|i| ListItem::new(i.to_string())).collect();
    let mut state = ListState::default().with_selected(Some(0));
    loop {
        term.draw(|f| {
            let size = f.area();

            // Calculate popup size (width and height)
            let width = (size.width / 2).min(50); // max width 50
            let height = (items.len() as u16 + 2).min(size.height / 2); // +2 for padding/border
            let x = (size.width.saturating_sub(width)) / 2;
            let y = (size.height.saturating_sub(height)) / 2;
            let rect = Rect::new(x, y, width, height);

            // Create list widget inside a block with border and title
            let list = List::new(list_items.clone())
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().bold())
                        .title(title)
                        .title_alignment(ratatui::layout::Alignment::Center),
                )
                .highlight_symbol(">> ")
                .highlight_style(
                    Style::default()
                        .fg(ratatui::style::Color::Green)
                        .add_modifier(Modifier::BOLD),
                );

            f.render_stateful_widget(list, rect, &mut state);
        })
        .ok()?;
        if let Some(bytes) = rx.recv().await {
            trace!("received input in widget");
            match Event::parse_from(&bytes) {
                Ok(None) => {
                    warn!("Couldn't fully parse bytes to terminal event");
                }
                Err(e) => {
                    error!("Couldn't parse bytes to terminal event: {e}");
                }
                Ok(Some(Event::Key(key_event))) => {
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
                            debug!("enter pressed");
                            term.backend_mut().execute(LeaveAlternateScreen).ok()?;
                            term.backend_mut().execute(Show).ok()?;
                            return state.selected();
                        }
                        Esc | Char('q') => {
                            debug!("esc or q pressed");
                            term.backend_mut().execute(LeaveAlternateScreen).ok()?;
                            term.backend_mut().execute(Show).ok()?;
                            return None;
                        }
                        _ => {}
                    }
                }
                Ok(Some(event)) => {
                    trace!("ignored event: {event:?}");
                }
            }
        }
    }
}

use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex, RwLock},
};

use bytes::Bytes;
use ratatui::{
    Frame,
    layout::Rect,
    prelude::*,
    style::{Modifier, Style},
    widgets::{Block, Borders, List, ListState},
};
use terminput::Event;
use tokio::{
    sync::{broadcast, mpsc, oneshot},
    task::LocalSet,
};

use crate::prelude::*;

pub struct Selector {
    pub task: Option<CliTask>,
    pub select_state: ListState,
    pub title: String,
    pub items: Vec<String>,
    pub tx: mpsc::Sender<Option<usize>>,
}
impl Selector {
    pub fn new(tx: mpsc::Sender<Option<usize>>) -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(Self {
            task: None,
            select_state: ListState::default().with_selected(Some(0)),
            title: "".to_owned(),
            items: Vec::new(),
            tx,
        }))
    }

    pub fn run<T: Into<String>>(
        selector: &Arc<RwLock<Self>>,
        mut rx: broadcast::Receiver<Bytes>,
        items: Vec<Box<dyn ToString + Send + Sync>>,
        title: T,
    ) -> Result<()> {
        let (start_tx, start_rx) = oneshot::channel();
        {
            let mut guard = selector.write().unwrap();
            if guard.task.is_some() {
                return Err(Error::Custom("duplicate task".to_owned()));
            }
            guard.items = items.into_iter().map(|x| x.to_string()).collect();
            guard.title = title.into();
            guard.task = Some(tokio::spawn({
                let selector = Arc::clone(selector);
                async move {
                    loop {
                        let key_event = {
                            if let Ok(bytes) = rx.recv().await {
                                match Event::parse_from(&bytes) {
                                    Ok(None) => {
                                        warn!("Couldn't fully parse bytes to terminal event");
                                        None
                                    }
                                    Err(e) => {
                                        error!("Couldn't parse bytes to terminal event: {e}");
                                        None
                                    }
                                    Ok(Some(Event::Key(key_event))) => Some(key_event),
                                    _ => None,
                                }
                            } else {
                                None
                            }
                        };

                        let tx = { selector.read().unwrap().tx.clone() };

                        let selection = {
                            if let Some(key_event) = key_event {
                                use terminput::KeyCode::*;
                                let mut guard = selector.write().unwrap();
                                match key_event.code {
                                    Up | Char('k') => {
                                        let i = match guard.select_state.selected() {
                                            Some(i) if i > 0 => i - 1,
                                            _ => 0,
                                        };
                                        guard.select_state.select(Some(i));
                                        None
                                    }
                                    Down | Char('j') => {
                                        let i = match guard.select_state.selected() {
                                            Some(i) if i < guard.items.len() - 1 => i + 1,
                                            _ => guard.items.len() - 1,
                                        };
                                        guard.select_state.select(Some(i));
                                        None
                                    }
                                    Enter => {
                                        debug!("enter pressed");
                                        Some(guard.select_state.selected())
                                    }
                                    Esc | Char('q') => {
                                        debug!("esc or q pressed");
                                        Some(None)
                                    }
                                    _ => None,
                                }
                            } else {
                                None
                            }
                        };

                        if let Some(selection) = selection {
                            tx.send(selection).await.unwrap();
                            break;
                        }
                    }
                    Ok(())
                }
            }));
        }
        start_tx.send(()).unwrap();
        Ok(())
    }

    pub fn render(selector: &Arc<RwLock<Self>>, f: &mut Frame) {
        let mut guard = selector.write().unwrap();
        let size = f.area();

        // Calculate popup size (width and height)
        let width = (size.width / 2).min(50); // max width 50
        let height = (guard.items.len() as u16 + 2).min(size.height / 2); // +2 for padding/border
        let x = (size.width.saturating_sub(width)) / 2;
        let y = (size.height.saturating_sub(height)) / 2;
        let rect = Rect::new(x, y, width, height);
        let list = List::new(guard.items.clone())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().bold())
                    .title(guard.title.clone())
                    .title_alignment(ratatui::layout::Alignment::Center),
            )
            .highlight_symbol(">> ")
            .highlight_style(
                Style::default()
                    .fg(ratatui::style::Color::Green)
                    .add_modifier(Modifier::BOLD),
            );

        f.render_stateful_widget(list, rect, &mut guard.select_state);
    }
}

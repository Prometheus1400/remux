use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use bytes::Bytes;
use fuzzy_matcher::{
    FuzzyMatcher,
    skim::{SkimMatcherV2, SkimScoreConfig},
};
use ratatui::{
    Frame,
    layout::Rect,
    prelude::*,
    style::{Modifier, Style},
    widgets::{Block, Borders, List, ListState, Paragraph},
};
use terminput::{Event, KeyCode};
use tokio::sync::{broadcast, mpsc};

use crate::{prelude::*, utils::DisplayableVec, widgets::traits::Selector};

#[derive(Debug, Clone)]
pub struct IndexedItem {
    pub index: usize,
    pub item: String,
}
impl IndexedItem {
    pub fn new(index: usize, item: String) -> Self {
        Self { index, item }
    }
}

pub struct FuzzySelector {
    pub select_state: ListState,
    pub title: Option<String>,
    pub items: Vec<IndexedItem>,
    pub filtered_items: Vec<IndexedItem>,
    pub tx: mpsc::Sender<Option<usize>>,
    pub is_running: bool,
    pub query: String,
}

impl FuzzySelector {
    pub fn new(tx: mpsc::Sender<Option<usize>>) -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(Self {
            select_state: ListState::default().with_selected(Some(0)),
            title: None,
            items: Vec::new(),
            filtered_items: Vec::new(),
            tx,
            is_running: false,
            query: "".to_owned(),
        }))
    }

    /// Helper to perform fuzzy filtering and update state
    fn filter_items(guard: &mut RwLockWriteGuard<'_, Self>, matcher: &SkimMatcherV2) {
        let mut filtered_items = guard
            .items
            .iter()
            .filter_map(|x| {
                matcher
                    .fuzzy_match(&x.item, &guard.query)
                    .map(|score| (score, x.clone()))
            })
            .collect::<Vec<(i64, IndexedItem)>>();

        filtered_items.sort_by(|a, b| b.0.cmp(&a.0));
        filtered_items.reverse();
        guard.filtered_items = filtered_items.into_iter().map(|x| x.1).collect();
    }
}

impl Selector for FuzzySelector {
    fn run<T: Into<String>>(
        selector: &Arc<RwLock<Self>>,
        mut rx: broadcast::Receiver<Bytes>,
        items: DisplayableVec,
        title: T,
    ) -> Result<()> {
        // --- Setup Phase ---
        {
            let mut guard = selector.write().unwrap();
            if guard.is_running {
                return Err(Error::Custom("duplicate task".to_owned()));
            }
            guard.items = items
                .to_strings()
                .into_iter()
                .enumerate()
                .map(|pair| IndexedItem::new(pair.0, pair.1))
                .collect();

            // Initial filter/setup
            guard.filtered_items = guard.items.clone();
            let opt = guard.filtered_items.last().map(|_| 0);
            guard.select_state.select(opt); // Select first item if exists

            guard.title = Some(title.into());
            guard.is_running = true;
            guard.query = String::new();
        } // Lock dropped

        // --- Spawn Asynchronous Task ---
        let selector_clone = Arc::clone(selector);
        tokio::spawn(async move {
            let matcher = SkimMatcherV2::default();

            loop {
                let final_selection: Option<Option<usize>> = {
                    if let Ok(bytes) = rx.recv().await {
                        // Task yields here, no lock held

                        match Event::parse_from(&bytes) {
                            Ok(Some(Event::Key(key_code))) => {
                                // Acquire lock to mutate state
                                let mut guard = selector_clone.write().unwrap();
                                let mut result: Option<Option<usize>> = None;

                                match key_code.code {
                                    KeyCode::Enter => {
                                        debug!("enter pressed");
                                        let selected_index = guard.select_state.selected();
                                        // Map the current filtered index to the original index
                                        let original_index = selected_index
                                            .and_then(|i| guard.filtered_items.get(i))
                                            .map(|item| item.index);

                                        result = Some(original_index); // Signal completion
                                    }
                                    KeyCode::Esc => {
                                        debug!("esc pressed");
                                        result = Some(None); // Signal cancellation
                                    }
                                    KeyCode::Up => {
                                        let current_index = guard.select_state.selected().unwrap_or(0);
                                        let i = current_index.saturating_sub(1);
                                        guard.select_state.select(Some(i));
                                    }
                                    KeyCode::Down => {
                                        let max_index = guard.filtered_items.len().saturating_sub(1);
                                        let current_index = guard.select_state.selected().unwrap_or(0);
                                        let i = (current_index + 1).min(max_index);
                                        guard.select_state.select(Some(i));
                                    }
                                    KeyCode::Backspace => {
                                        guard.query.pop();
                                        Self::filter_items(&mut guard, &matcher);
                                        let opt = guard.filtered_items.last().map(|_| 0);
                                        guard.select_state.select(opt);
                                    }
                                    KeyCode::Char(c) => {
                                        guard.query.push(c);
                                        Self::filter_items(&mut guard, &matcher);
                                        let opt = guard.filtered_items.last().map(|_| 0);
                                        guard.select_state.select(opt);
                                    }
                                    _ => {}
                                }

                                // CRITICAL: Explicitly drop the guard before any subsequent await
                                drop(guard);

                                result
                            }
                            Ok(None) => {
                                warn!("Couldn't fully parse bytes to terminal event");
                                None
                            }
                            Err(e) => {
                                error!("Couldn_t parse bytes to terminal event: {e}");
                                None
                            }
                            _ => None,
                        }
                    } else {
                        // rx channel closed
                        break;
                    }
                };

                // If a final selection was determined (Enter/Esc), send the result and break.
                if let Some(selection) = final_selection {
                    // Acquire a read lock just to get a clone of the Sender (tx)
                    let tx_clone = selector_clone.read().unwrap().tx.clone();
                    // Lock is dropped immediately after clone

                    tx_clone.send(selection).await.unwrap();
                    break;
                }
            } // end loop

            // --- Cleanup Phase ---
            {
                // Re-acquire lock to set state to finished
                selector_clone.write().unwrap().is_running = false;
            }
            Ok::<(), Error>(())
        });
        Ok(())
    }

    fn render(selector: &Arc<RwLock<Self>>, f: &mut Frame) {
        // Use read lock for synchronous, non-mutating rendering
        let guard = selector.read().unwrap();
        let size = f.area();

        // Calculate popup size
        let width = (size.width / 2).min(50);
        let height = (guard.filtered_items.len() as u16 + 8).min(size.height / 2);
        let x = (size.width.saturating_sub(width)) / 2;
        let y = (size.height.saturating_sub(height)) / 2;
        let rect = Rect::new(x, y, width, height);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(3)])
            .split(rect);

        let list = List::new(
            guard
                .filtered_items
                .iter()
                .map(|pair| pair.item.clone())
                .collect::<Vec<String>>(),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().bold())
                .title(guard.title.as_ref().unwrap_or(&String::from("Select Item")).clone())
                .title_alignment(ratatui::layout::Alignment::Center),
        )
        .highlight_symbol(">> ")
        .highlight_style(
            Style::default()
                .fg(ratatui::style::Color::Green)
                .add_modifier(Modifier::BOLD),
        );

        // Temporarily mutable state for rendering list
        let mut list_state = guard.select_state.clone();
        f.render_stateful_widget(list, chunks[0], &mut list_state);

        let paragraph = Paragraph::new(guard.query.clone()).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().bold()),
        );

        f.render_widget(paragraph, chunks[1]);
    }
}

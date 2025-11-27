use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use bytes::Bytes;
use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};
use ratatui::{
    Frame,
    layout::Rect,
    widgets::{ListState, Padding},
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
}

impl Selector for FuzzySelector {
    fn run<T: Into<String>>(
        selector: &Arc<RwLock<Self>>,
        mut rx: broadcast::Receiver<Bytes>,
        items: DisplayableVec,
        title: T,
    ) -> Result<()> {
        let matcher = SkimMatcherV2::default();
        {
            let mut guard = selector.write().unwrap();
            if guard.is_running {
                return Err(Error::Custom("duplicate task".to_owned()));
            }
            guard.is_running = true;
            guard.items = items
                .to_strings()
                .into_iter()
                .enumerate()
                .map(|pair| IndexedItem::new(pair.0, pair.1))
                .collect();
            guard.filtered_items = filter_items(&matcher, &guard.query, &guard.items);
            let opt = Some(guard.filtered_items.len().saturating_sub(1));
            guard.select_state.select(opt); // Select first item if exists
            guard.title = Some(title.into());
            guard.query = String::new();
        }

        let selector_clone = Arc::clone(selector);
        tokio::spawn(async move {
            loop {
                let final_selection: Option<Option<usize>> = {
                    if let Ok(bytes) = rx.recv().await {
                        match Event::parse_from(&bytes) {
                            Ok(Some(Event::Key(key_code))) => {
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

                                        result = Some(original_index);
                                    }
                                    KeyCode::Esc => {
                                        debug!("esc pressed");
                                        result = Some(None);
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
                                        guard.filtered_items = filter_items(&matcher, &guard.query, &guard.items);
                                        let opt = Some(guard.filtered_items.len().saturating_sub(1));
                                        guard.select_state.select(opt);
                                    }
                                    KeyCode::Char(c) => {
                                        guard.query.push(c);
                                        guard.filtered_items = filter_items(&matcher, &guard.query, &guard.items);
                                        let opt = Some(guard.filtered_items.len().saturating_sub(1));
                                        guard.select_state.select(opt);
                                    }
                                    _ => {}
                                }

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
                        break;
                    }
                };

                // If a final selection was determined (Enter/Esc), send the result and break.
                if let Some(selection) = final_selection {
                    let tx_clone = selector_clone.read().unwrap().tx.clone();
                    tx_clone.send(selection).await.unwrap();
                    break;
                }
            }

            {
                selector_clone.write().unwrap().is_running = false;
            }
            Ok::<(), Error>(())
        });
        Ok(())
    }

    fn render(selector: &Arc<RwLock<Self>>, f: &mut Frame) {
        use ratatui::{
            prelude::*,
            style::{Modifier, Style},
            widgets::{Block, Borders, List, Paragraph},
        };
        let (title, query, items, mut select_state) = {
            let guard = selector.read().unwrap();
            (
                guard.title.as_ref().unwrap_or(&"Select Item".to_owned()).clone(),
                guard.query.clone(),
                guard
                    .filtered_items
                    .iter()
                    .map(|pair| pair.item.clone())
                    .collect::<Vec<String>>(),
                guard.select_state.clone(),
            )
        };
        let rect = get_rect(f);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(3)])
            .split(rect);

        let max_height = chunks[0].height;
        let items_height = items.len() as u16 + 2;
        let subchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(max_height - items_height),
                Constraint::Length(items_height),
            ])
            .split(chunks[0]);

        let list = List::new(items)
            .block(Block::default().padding(Padding::uniform(1)))
            .highlight_symbol(">> ")
            .highlight_style(
                Style::default()
                    .fg(ratatui::style::Color::Green)
                    .add_modifier(Modifier::BOLD),
            );

        let display_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().bold())
            .title(title)
            .title_alignment(ratatui::layout::Alignment::Center);

        let display_query = Paragraph::new(query).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().bold())
                .padding(Padding::horizontal(1)),
        );

        f.render_widget(display_block, chunks[0]);
        f.render_stateful_widget(list, subchunks[1], &mut select_state);
        f.render_widget(display_query, chunks[1]);
    }
}

fn filter_items(matcher: &SkimMatcherV2, query: &str, items: &[IndexedItem]) -> Vec<IndexedItem> {
    let mut filtered_items = items
        .iter()
        .filter_map(|x| matcher.fuzzy_match(&x.item, query).map(|score| (score, x.clone())))
        .collect::<Vec<(i64, IndexedItem)>>();

    filtered_items.sort_by(|a, b| b.0.cmp(&a.0));
    filtered_items.reverse();
    filtered_items.into_iter().map(|x| x.1).collect::<Vec<IndexedItem>>()
}

fn get_rect(f: &Frame) -> Rect {
    let size = f.area();
    let width = size.width / 2;
    let height = size.height / 2;
    let x = (size.width.saturating_sub(width)) / 2;
    let y = (size.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}

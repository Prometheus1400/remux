use std::sync::{Arc, RwLock};

use bytes::Bytes;
use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};
use ratatui::{Frame, widgets::ListState};
use terminput::Event;
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

#[derive(Debug, Clone)]
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
            guard.filtered_items = guard.items.clone();
            guard.title = Some(title.into());
        }
        tokio::spawn({
            let selector = Arc::clone(selector);
            let matcher = SkimMatcherV2::default();
            {
                selector.write().unwrap().is_running = true;
            }
            async move {
                loop {
                    let selection = {
                        if let Ok(bytes) = rx.recv().await {
                            debug!("bytes gotten in fuzzy selector");
                            use terminput::KeyCode::*;
                            match Event::parse_from(&bytes) {
                                Ok(None) => {
                                    warn!("Couldn't fully parse bytes to terminal event");
                                    None
                                }
                                Err(e) => {
                                    error!("Couldn't parse bytes to terminal event: {e}");
                                    None
                                }
                                Ok(Some(Event::Key(key_code))) => {
                                    let mut guard = selector.write().unwrap();
                                    match key_code.code {
                                        Enter => {
                                            debug!("enter pressed");
                                            Some(ListState::default().with_selected(Some(0)).selected())
                                            // Some(guard.select_state.selected())
                                        }
                                        Esc => {
                                            debug!("esc pressed");
                                            Some(None)
                                        }
                                        Up => {
                                            todo!()
                                            // let i = match guard.select_state.selected() {
                                            //     Some(i) if i > 0 => i - 1,
                                            //     _ => 0,
                                            // };
                                            // guard.select_state.select(Some(i));
                                            // None
                                        }
                                        Down => {
                                            todo!()
                                            // let i = match guard.select_state.selected() {
                                            //     Some(i) if i < guard.items.len() - 1 => i + 1,
                                            //     _ => guard.items.len() - 1,
                                            // };
                                            // guard.select_state.select(Some(i));
                                            // None
                                        }
                                        Backspace => {
                                            guard.query.pop();
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
                                            guard.filtered_items = filtered_items.into_iter().map(|x| x.1).collect();
                                            None
                                        }
                                        Char(c) => {
                                            guard.query.push(c);
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
                                            guard.filtered_items = filtered_items.into_iter().map(|x| x.1).collect();
                                            None
                                        }
                                        _ => None,
                                    }
                                }
                                _ => None,
                            }
                        } else {
                            None
                        }
                    };

                    let tx = { selector.read().unwrap().tx.clone() };
                    if let Some(selection) = selection {
                        tx.send(selection).await.unwrap();
                        break;
                    }
                }
                {
                    selector.write().unwrap().is_running = false;
                }
                Ok::<(), Error>(())
            }
        });
        Ok(())
    }

    fn render(selector: &Arc<RwLock<Self>>, f: &mut Frame) {
        use ratatui::{
            layout::Rect,
            prelude::*,
            style::{Modifier, Style},
            widgets::{Block, Borders, List, Paragraph},
        };
        let mut guard = selector.write().unwrap();
        let size = f.area();

        // Calculate popup size (width and height)
        let width = (size.width / 2).min(50); // max width 50
        let height = (guard.filtered_items.len() as u16 + 8).min(size.height / 2); // +2 for padding/border
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
                .title(guard.title.as_ref().unwrap().clone())
                .title_alignment(ratatui::layout::Alignment::Center),
        )
        .highlight_symbol(">> ")
        .highlight_style(
            Style::default()
                .fg(ratatui::style::Color::Green)
                .add_modifier(Modifier::BOLD),
        );

        f.render_stateful_widget(list, chunks[0], &mut guard.select_state);

        let paragraph = Paragraph::new(guard.query.clone()).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().bold()),
        );

        f.render_widget(paragraph, chunks[1]);
    }
}

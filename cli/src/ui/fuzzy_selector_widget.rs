use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};
use ratatui::widgets::{Padding, StatefulWidget};

use crate::{
    app::{IndexedItem, SelectorState},
    prelude::*,
    ui::traits::{Selection, SelectorStatefulWidget},
};

#[derive(Debug, Default)]
pub struct FuzzySelectorWidget {}

impl StatefulWidget for FuzzySelectorWidget {
    type State = SelectorState;

    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer, state: &mut Self::State) {
        use ratatui::{
            prelude::*,
            style::{Modifier, Style},
            widgets::{Block, Borders, List, Paragraph},
        };

        let list_state = &mut state.list_state;
        let filtered_items = state
            .displaying_list
            .iter()
            .map(|pair| pair.item.clone())
            .collect::<Vec<String>>();
        let query = &state.query;

        let width = area.width / 2;
        let height = area.height / 2;
        let x = (area.width.saturating_sub(width)) / 2;
        let y = (area.height.saturating_sub(height)) / 2;
        let rect = Rect::new(x, y, width, height);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(3)])
            .split(rect);

        let max_height = chunks[0].height;
        let items_height = filtered_items.len() as u16 + 2;
        let subchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(max_height - items_height),
                Constraint::Length(items_height),
            ])
            .split(chunks[0]);

        let list = List::new(filtered_items)
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
            .title("Selecting")
            .title_alignment(ratatui::layout::Alignment::Center);

        let display_query = Paragraph::new(query.clone()).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().bold())
                .padding(Padding::horizontal(1)),
        );

        display_block.render(chunks[0], buf);
        StatefulWidget::render(&list, subchunks[1], buf, list_state);
        display_query.render(chunks[1], buf);
    }
}

impl SelectorStatefulWidget for FuzzySelectorWidget {
    fn input(event: terminput::Event, state: &mut Self::State) -> Option<Selection> {
        use terminput::KeyCode::*;

        let list_state = &mut state.list_state;
        let list = state
            .list
            .iter()
            .enumerate()
            .map(|(i, x)| IndexedItem::new(i, x.clone()))
            .collect::<Vec<IndexedItem>>();
        let filtered_list = &mut state.displaying_list;
        let query = &mut state.query;
        let matcher = SkimMatcherV2::default();

        match event {
            terminput::Event::Key(key_event) => match key_event.code {
                Enter => {
                    debug!("enter pressed");
                    let selected_index = list_state.selected();
                    // Map the current filtered index to the original index
                    let original_index = selected_index
                        .and_then(|i| filtered_list.get(i))
                        .map(|item| item.index)?;

                    Some(Selection::Index(original_index))
                }
                Esc => {
                    debug!("esc pressed");
                    Some(Selection::Cancelled)
                }
                Up => {
                    let current_index = list_state.selected().unwrap_or(0);
                    let i = current_index.saturating_sub(1);
                    list_state.select(Some(i));
                    None
                }
                Down => {
                    let max_index = filtered_list.len().saturating_sub(1);
                    let current_index = list_state.selected().unwrap_or(0);
                    let i = (current_index + 1).min(max_index);
                    list_state.select(Some(i));
                    None
                }
                Backspace => {
                    query.pop();
                    *filtered_list = filter_items(&matcher, query, &list);
                    let opt = Some(filtered_list.len().saturating_sub(1));
                    list_state.select(opt);
                    None
                }
                Char(c) => {
                    query.push(c);
                    *filtered_list = filter_items(&matcher, query, &list);
                    let opt = Some(filtered_list.len().saturating_sub(1));
                    list_state.select(opt);
                    None
                }
                _ => None,
            },
            _ => None,
        }
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

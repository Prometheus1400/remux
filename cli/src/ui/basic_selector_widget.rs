use ratatui::widgets::StatefulWidget;

use crate::{
    app::SelectorState,
    prelude::*,
    ui::traits::{Selection, SelectorStatefulWidget},
};

#[derive(Debug, Default)]
pub struct BasicSelectorWidget {}

impl StatefulWidget for BasicSelectorWidget {
    type State = SelectorState;

    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer, state: &mut Self::State) {
        use ratatui::{
            layout::Rect,
            prelude::Stylize,
            style::{Modifier, Style},
            widgets::{Block, Borders, List},
        };
        let list_state = &mut state.list_state;
        let list = &state.list;

        // Calculate popup size (width and height)
        let width = (area.width / 2).min(50); // max width 50
        let height = (list.len() as u16 + 2).min(area.height / 2); // +2 for padding/border
        let x = (area.width.saturating_sub(width)) / 2;
        let y = (area.height.saturating_sub(height)) / 2;
        let rect = Rect::new(x, y, width, height);
        let list = List::new(list.clone())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().bold())
                    .title("Select")
                    .title_alignment(ratatui::layout::Alignment::Center),
            )
            .highlight_symbol(">> ")
            .highlight_style(
                Style::default()
                    .fg(ratatui::style::Color::Green)
                    .add_modifier(Modifier::BOLD),
            );

        list.render(rect, buf, list_state);
    }
}

impl SelectorStatefulWidget for BasicSelectorWidget {
    fn input(event: terminput::Event, state: &mut Self::State) -> Option<Selection> {
        use terminput::KeyCode::*;

        let list_state = &mut state.list_state;
        let list = &state.list;

        match event {
            terminput::Event::Key(key_event) => match key_event.code {
                Up | Char('k') => {
                    let i = match list_state.selected() {
                        Some(i) if i > 0 => i - 1,
                        _ => 0,
                    };
                    list_state.select(Some(i));
                    None
                }
                Down | Char('j') => {
                    let i = match list_state.selected() {
                        Some(i) if i < list.len() - 1 => i + 1,
                        _ => list.len() - 1,
                    };
                    list_state.select(Some(i));
                    None
                }
                Enter => {
                    debug!("enter pressed");
                    list_state.selected().map(Selection::Index)
                }
                Esc | Char('q') => {
                    debug!("esc or q pressed");
                    Some(Selection::Cancelled)
                }
                _ => None,
            },
            _ => None,
        }
    }
}

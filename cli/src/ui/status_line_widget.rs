use ratatui::{
    layout::{Constraint, Direction, Layout},
    widgets::{Paragraph, Widget},
};
use tracing::info;

use crate::states::status_line_state::StatusLineState;

pub struct StatusLineWidget {
    state: StatusLineState,
}

impl StatusLineWidget {
    pub fn new(state: StatusLineState) -> Self {
        Self { state }
    }
}

impl Widget for StatusLineWidget {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer) {
        if !self.state.enabled || area.height < 1 {
            return;
        }

        info!("rendering status line widget");

        // 1. Create 3 horizontal constraints for Left, Center, Right sections
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(area.width / 3), // Fixed or Length for A
                Constraint::Min(0),                 // Remaining space for B (Center)
                Constraint::Length(area.width / 3), // Fixed or Length for C
            ])
            .split(area);

        let separator = " | ";
        let text_a = self.state.a.join(separator);
        let text_b = self.state.b.join(separator);
        let text_c = self.state.c.join(separator);
        let paragraph_a = Paragraph::new(text_a);
        paragraph_a.render(chunks[0], buf);
        let paragraph_b = Paragraph::new(text_b).alignment(ratatui::layout::Alignment::Center);
        paragraph_b.render(chunks[1], buf);
        let paragraph_c = Paragraph::new(text_c).alignment(ratatui::layout::Alignment::Right);
        paragraph_c.render(chunks[2], buf);
    }
}

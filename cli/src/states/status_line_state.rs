use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    widgets::Paragraph,
};

#[derive(Debug, Clone)]
pub struct StatusLineState {
    pub enabled: bool,
    pub a: Vec<String>,
    pub b: Vec<String>,
    pub c: Vec<String>,
}

impl Default for StatusLineState {
    fn default() -> Self {
        Self {
            enabled: true,
            a: Default::default(),
            b: Default::default(),
            c: Default::default(),
        }
    }
}
impl StatusLineState {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }
}

impl StatusLineState {
    pub fn render(&self, f: &mut Frame, area: Rect) {
        if !self.enabled || area.height < 1 {
            return;
        }

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
        let text_a = self.a.join(separator);
        let text_b = self.b.join(separator);
        let text_c = self.c.join(separator);
        let paragraph_a = Paragraph::new(text_a);
        f.render_widget(paragraph_a, chunks[0]);
        let paragraph_b = Paragraph::new(text_b).alignment(ratatui::layout::Alignment::Center);
        f.render_widget(paragraph_b, chunks[1]);
        let paragraph_c = Paragraph::new(text_c).alignment(ratatui::layout::Alignment::Right);
        f.render_widget(paragraph_c, chunks[2]);
    }
}

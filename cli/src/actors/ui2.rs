use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
};
use tui_term::widget::PseudoTerminal;

use crate::app::AppState;

pub fn draw(f: &mut Frame, state: &mut AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // pseudo terminal takes everything else
            Constraint::Length(1), // bottom status bar
        ])
        .split(f.area());

    // render the normal terminal output
    let term_ui = PseudoTerminal::new(state.terminal.emulator.screen());
    f.render_widget(term_ui, chunks[0]);

    // render the status bar
    // let status_line = StatusLine::new(app.status_line);
    // f.render_widget();

    // render selector if active
    // if self.ui_state == UIState::SelectingBasic {
    //     BasicSelector::render(&self.basic_selector, f);
    // }
    // if self.ui_state == UIState::SelectingFuzzy {
    //     FuzzySelector::render(&self.fuzzy_selector, f);
    // }
}

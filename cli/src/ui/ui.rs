use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
};
use tui_term::widget::PseudoTerminal;

use crate::{
    app::{AppMode, AppState},
    prelude::*,
    ui::{
        basic_selector_widget::BasicSelectorWidget, fuzzy_selector_widget::FuzzySelectorWidget,
        status_line_widget::StatusLineWidget,
    },
};

#[instrument(skip(f))]
pub fn draw(f: &mut Frame, state: &mut AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // pseudo terminal takes everything else
            Constraint::Length(1), // bottom status bar
        ])
        .split(f.area());

    // render the normal terminal output
    let term_area = chunks[0];
    state.terminal.size = (term_area.height, term_area.width);
    trace!("rendering terminal into rect: {term_area}");
    let term_ui = PseudoTerminal::new(state.terminal.emulator.screen());
    f.render_widget(term_ui, term_area);

    // render the status bar
    let status_line = StatusLineWidget::new(state.ui.status_line.clone());
    f.render_widget(status_line, chunks[1]);

    if let AppMode::SelectingSession = state.mode {
        match state.ui.selector.selector_type {
            crate::app::SelectorType::Basic => {
                let popup = BasicSelectorWidget::default();
                f.render_stateful_widget(popup, f.area(), &mut state.ui.selector);
            }
            crate::app::SelectorType::Fuzzy => {
                let popup = FuzzySelectorWidget::default();
                f.render_stateful_widget(popup, f.area(), &mut state.ui.selector);
            }
        }
    }
}

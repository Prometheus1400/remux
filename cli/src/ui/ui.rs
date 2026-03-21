use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
};
use remux_core::states::ServerSnapshot;
use tui_term::widget::PseudoTerminal;

use crate::{
    app::{AppMode, TerminalState, UiState},
    prelude::*,
    ui::{
        basic_selector_widget::BasicSelectorWidget, fuzzy_selector_widget::FuzzySelectorWidget,
        status_line_widget::StatusLineWidget,
    },
};

#[instrument(skip(f))]
pub fn draw(
    f: &mut Frame,
    _server: &ServerSnapshot,
    terminal: &TerminalState,
    ui_state: &mut UiState,
    status_line_state: crate::states::status_line_state::StatusLineState,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // pseudo terminal takes everything else
            Constraint::Length(1), // bottom status bar
        ])
        .split(f.area());

    // render the normal terminal output
    let term_area = chunks[0];
    trace!("rendering terminal into rect: {term_area}");
    let term_ui = PseudoTerminal::new(terminal.emulator.screen());
    f.render_widget(term_ui, term_area);

    // render the status bar
    let status_line = StatusLineWidget::new(status_line_state);
    f.render_widget(status_line, chunks[1]);

    if let AppMode::SelectingSession = ui_state.mode {
        match ui_state.selector.selector_type {
            crate::app::SelectorType::Basic => {
                let popup = BasicSelectorWidget::default();
                f.render_stateful_widget(popup, f.area(), &mut ui_state.selector);
            }
            crate::app::SelectorType::Fuzzy => {
                let popup = FuzzySelectorWidget::default();
                f.render_stateful_widget(popup, f.area(), &mut ui_state.selector);
            }
        }
    }
}

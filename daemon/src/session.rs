use crate::pane::{Focused, Hidden, Pane, PaneBuilder};

pub struct GlobalSessionState {
    active_session: Session<Focused>,
    inactive_sessions: Vec<Session<Hidden>>,
}

impl GlobalSessionState {
    pub fn new_session(&mut self) {

    } 
}

// TODO: right now each session just has one pane
pub struct Session<State> {
    pane: Pane<State>,
    _state: std::marker::PhantomData<State>
}

impl Session<Focused> {
    pub fn new() -> Self {
        Self {
            pane: PaneBuilder::new(history_size, live_bytes_tx)
        } 
    }
    pub fn hide(self) -> Session<Hidden> {
        Session::<Hidden> {
            pane: self.pane.hide(),
            _state: std::marker::PhantomData
        }
    }
}

impl Session<Hidden> {
    pub fn focus(self) -> Session<Focused> {
        Session::<Focused> {
            pane: self.pane.focus(),
            _state: std::marker::PhantomData
        }
    }
}

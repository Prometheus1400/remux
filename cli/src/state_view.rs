// clients view of the state
pub struct StateView {
    session_ids: Vec<u32>,
    active_session: Option<u32>,

    window_ids: Vec<u32>,
    active_window: Option<u32>,
}

impl Default for StateView {
    fn default() -> Self {
        Self {
            session_ids: vec![],
            active_session: None,
            window_ids: vec![],
            active_window: None,
        }
    }
}

impl StateView {
    pub fn add_session(&mut self, session_id: u32) {
        todo!()
    }
    pub fn remove_session(&mut self, session_id: u32) {
        todo!()
    }
    pub fn set_active_session(&mut self, session_id: u32) {
        todo!()
    }
    pub fn add_window(&mut self, window_id: u32) {
        todo!()
    }
    pub fn remove_window(&mut self, window_id: u32) {
        todo!()
    }
    pub fn set_active_window(&mut self, window_id: u32) {
        todo!()
    }
}

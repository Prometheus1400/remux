// clients view of the state
#[derive(Debug, Clone)]
pub struct StateView {
    pub session_ids: Vec<u32>,
    pub active_session: Option<u32>,

    // window_ids: Vec<u32>,
    // pub active_window: Option<u32>,
}

impl Default for StateView {
    fn default() -> Self {
        Self {
            session_ids: vec![],
            active_session: None,
            // window_ids: vec![],
            // active_window: None,
        }
    }
}

impl StateView {
    pub fn set_sessions(&mut self, session_ids: Vec<u32>) {
        self.session_ids = session_ids;
    }
    pub fn add_session(&mut self, session_id: u32) {
        self.session_ids.push(session_id);
    }
    pub fn remove_session(&mut self, session_id: u32) {
        self.session_ids.retain(|s| s != &session_id);
    }
    pub fn set_active_session(&mut self, session_id: u32) {
        self.active_session = Some(session_id);
    }
    // pub fn add_window(&mut self, window_id: u32) {
    //     todo!()
    // }
    // pub fn remove_window(&mut self, window_id: u32) {
    //     todo!()
    // }
    // pub fn set_active_window(&mut self, window_id: u32) {
    //     todo!()
    // }
}

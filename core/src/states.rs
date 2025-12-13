/// comprehensive summary of the state of the daemon
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SessionInfo {
    pub id: u32,
    pub name: String,
}

#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DaemonState {
    pub sessions: Vec<SessionInfo>,
    pub active_session: Option<u32>,
    // window_ids: Vec<u32>,
    // pub active_window: Option<u32>,
}

impl DaemonState {
    pub fn set_sessions(&mut self, sessions: Vec<(u32, String)>) {
        self.sessions = sessions
            .into_iter()
            .map(|(id, name)| SessionInfo { id, name })
            .collect();
    }
    pub fn add_session(&mut self, id: u32, name: String) {
        let i = self
            .sessions
            .binary_search_by_key(&id, |info| info.id)
            .unwrap_or_else(|i| i);
        self.sessions.insert(i, SessionInfo { id, name });
    }
    // pub fn remove_session(&mut self, session_id: u32) {
    //     self.session_ids.retain(|s| s != &session_id);
    // }
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

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum CliEvent {
    Raw { bytes: Vec<u8> },            // raw user keypresses
    SwitchSession { session_id: u32 }, // switch session - does nothing if session does not exist
}

#[derive(Debug, Serialize, Deserialize)]
pub enum DaemonEvent {
    Raw { bytes: Vec<u8> }, // raw response - ansii control chars
    SwitchSessionOptions { session_ids: Vec<u32> },
}

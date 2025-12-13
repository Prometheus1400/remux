use bytes::Bytes;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum CliEvent {
    Raw(Bytes), // raw user keypresses

    // pane related
    KillPane,
    NextPane,
    SplitPaneVertical,
    SplitPaneHorizontal,
    PrevPane,

    SwitchSession(u32), // switch session - does nothing if session does not exist

    TerminalResize { rows: u16, cols: u16 },

    Detach,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum DaemonEvent {
    Raw(Bytes), // raw response - ansii control chars

    // session events
    CurrentSessions(Vec<u32>),
    ActiveSession(u32),
    NewSession(u32, String),
    DeletedSession(u32),
    // TODO: for window id
    Disconnected,
}

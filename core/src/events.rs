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

    OpenSessionSwitcher,

    TerminalResize { rows: u16, cols: u16 },

    Detach,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum DaemonEvent {
    Raw(Bytes), // raw response - ansii control chars
    Disconnected,
}

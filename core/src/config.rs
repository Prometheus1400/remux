use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyBinding {
    pub sequence: Vec<u8>,
    pub action: KeyAction,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum KeyAction {
    Builtin(BuiltinAction),
    Named(String),
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum BuiltinAction {
    SplitPaneVertical,
    SplitPaneHorizontal,
    FocusPaneLeft,
    FocusPaneDown,
    FocusPaneUp,
    FocusPaneRight,
    KillPane,
    Detach,
    OpenSessionSwitcher,
}

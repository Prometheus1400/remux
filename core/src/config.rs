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
    NewWindow,
    NextWindow,
    PrevWindow,
    KillWindow,
    SelectWindow1,
    SelectWindow2,
    SelectWindow3,
    SelectWindow4,
    SelectWindow5,
    SelectWindow6,
    SelectWindow7,
    SelectWindow8,
    SelectWindow9,
    Detach,
    OpenSessionSwitcher,
}

impl BuiltinAction {
    pub const ALL: [Self; 22] = [
        Self::SplitPaneVertical,
        Self::SplitPaneHorizontal,
        Self::FocusPaneLeft,
        Self::FocusPaneDown,
        Self::FocusPaneUp,
        Self::FocusPaneRight,
        Self::KillPane,
        Self::NewWindow,
        Self::NextWindow,
        Self::PrevWindow,
        Self::KillWindow,
        Self::SelectWindow1,
        Self::SelectWindow2,
        Self::SelectWindow3,
        Self::SelectWindow4,
        Self::SelectWindow5,
        Self::SelectWindow6,
        Self::SelectWindow7,
        Self::SelectWindow8,
        Self::SelectWindow9,
        Self::Detach,
        Self::OpenSessionSwitcher,
    ];

    pub fn config_name(self) -> &'static str {
        match self {
            Self::SplitPaneVertical => "split-pane-vertical",
            Self::SplitPaneHorizontal => "split-pane-horizontal",
            Self::FocusPaneLeft => "focus-pane-left",
            Self::FocusPaneDown => "focus-pane-down",
            Self::FocusPaneUp => "focus-pane-up",
            Self::FocusPaneRight => "focus-pane-right",
            Self::KillPane => "kill-pane",
            Self::NewWindow => "new-window",
            Self::NextWindow => "next-window",
            Self::PrevWindow => "prev-window",
            Self::KillWindow => "kill-window",
            Self::SelectWindow1 => "select-window-1",
            Self::SelectWindow2 => "select-window-2",
            Self::SelectWindow3 => "select-window-3",
            Self::SelectWindow4 => "select-window-4",
            Self::SelectWindow5 => "select-window-5",
            Self::SelectWindow6 => "select-window-6",
            Self::SelectWindow7 => "select-window-7",
            Self::SelectWindow8 => "select-window-8",
            Self::SelectWindow9 => "select-window-9",
            Self::Detach => "detach",
            Self::OpenSessionSwitcher => "open-session-switcher",
        }
    }

    pub fn from_config_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.config_name() == name)
    }
}

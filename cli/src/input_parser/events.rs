use remux_core::events::CliEvent;

pub enum ParsedEvent {
    LocalAction(LocalAction),
    DaemonAction(CliEvent),
}

pub enum LocalAction {
    SwitchSession,
}

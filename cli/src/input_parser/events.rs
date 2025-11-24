use remux_core::events::CliEvent;

pub enum ParsedEvent {
    LocalAction(Action),
    DaemonAction(CliEvent),
}

pub enum Action {
    SwitchSession,
}

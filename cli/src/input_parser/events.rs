use remux_core::events::CliEvent;

#[derive(Debug)]
pub enum ParsedEvent {
    DaemonAction(CliEvent),
}

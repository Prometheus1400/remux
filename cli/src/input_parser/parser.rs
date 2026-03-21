use bytes::Bytes;
use remux_core::events::CliEvent;

use crate::{input_parser::events::ParsedEvent, prelude::*};

#[allow(unused)]
const CTRL_SPACE: u8 = 0x00;
const CTRL_B: u8 = 0x02;
const PERCENT: u8 = 0x25;
const DOUBLE_QUOTE: u8 = 0x22;
const N: u8 = 0x6E;
const P: u8 = 0x70;
const S: u8 = 0x73;
const X: u8 = 0x78;
const D: u8 = 0x64;

#[derive(Debug, Default)]
pub struct InputParser {
    buf: Vec<u8>,
}

impl InputParser {
    pub fn process(&mut self, input: &[u8]) -> Vec<ParsedEvent> {
        use ParsedEvent::DaemonAction;
        self.buf.extend(input);
        let mut events = vec![];
        let mut i = 0;
        while i < self.buf.len() {
            let b = self.buf[i];
            match b {
                CTRL_B => {
                    trace!("prefix detected");
                    if (i + 1) < self.buf.len() {
                        let b_next = self.buf[i + 1];
                        if i > 0 {
                            let old: Vec<u8> = self.buf.drain(..i).collect();
                            events.push(DaemonAction(CliEvent::Raw(Bytes::from(old))));
                        }
                        match b_next {
                            PERCENT => {
                                events.push(DaemonAction(CliEvent::SplitPaneVertical));
                                self.buf.drain(..2);
                            }
                            DOUBLE_QUOTE => {
                                events.push(DaemonAction(CliEvent::SplitPaneHorizontal));
                                self.buf.drain(..2);
                            }
                            N => {
                                events.push(DaemonAction(CliEvent::NextPane));
                                self.buf.drain(..2);
                            }
                            P => {
                                events.push(DaemonAction(CliEvent::PrevPane));
                                self.buf.drain(..2);
                            }
                            X => {
                                events.push(DaemonAction(CliEvent::KillPane));
                                self.buf.drain(..2);
                            }
                            D => {
                                events.push(DaemonAction(CliEvent::Detach));
                                self.buf.drain(..2);
                            }
                            S => {
                                events.push(DaemonAction(CliEvent::OpenSessionSwitcher));
                                self.buf.drain(..2);
                            }
                            _ => {
                                self.buf.drain(..2);
                            }
                        }
                        i = 0;
                    } else {
                        let old: Vec<u8> = self.buf.drain(..i).collect();
                        if !old.is_empty() {
                            events.push(DaemonAction(CliEvent::Raw(Bytes::from(old))));
                        }
                        i = 0;
                        break;
                    }
                }
                _ => i += 1,
            }
        }

        let old: Vec<u8> = self.buf.drain(..i).collect();
        if !old.is_empty() {
            events.push(DaemonAction(CliEvent::Raw(Bytes::from(old))));
        }
        trace!("return from process with remaining {:?}", self.buf);
        events
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use remux_core::events::CliEvent;

    use super::InputParser;
    use crate::input_parser::ParsedEvent;

    fn assert_raw(event: &ParsedEvent, expected: &[u8]) {
        match event {
            ParsedEvent::DaemonAction(CliEvent::Raw(bytes)) => assert_eq!(bytes, &Bytes::copy_from_slice(expected)),
            other => panic!("expected raw event, got {other:?}"),
        }
    }

    fn assert_command(event: &ParsedEvent, predicate: impl FnOnce(&CliEvent) -> bool) {
        match event {
            ParsedEvent::DaemonAction(cli_event) => assert!(predicate(cli_event), "unexpected event: {cli_event:?}"),
        }
    }

    #[test]
    fn raw_input_passes_through_without_prefix() {
        let mut parser = InputParser::default();

        let events = parser.process(b"hello");

        assert_eq!(events.len(), 1);
        assert_raw(&events[0], b"hello");
    }

    #[test]
    fn prefix_split_across_calls_is_buffered() {
        let mut parser = InputParser::default();

        let first = parser.process(&[0x02]);
        let second = parser.process(b"n");

        assert!(first.is_empty());
        assert_eq!(second.len(), 1);
        assert_command(&second[0], |event| matches!(event, CliEvent::NextPane));
    }

    #[test]
    fn raw_before_prefix_is_emitted_before_command() {
        let mut parser = InputParser::default();

        let events = parser.process(b"ab\x02d");

        assert_eq!(events.len(), 2);
        assert_raw(&events[0], b"ab");
        assert_command(&events[1], |event| matches!(event, CliEvent::Detach));
    }

    #[test]
    fn unknown_prefix_command_is_dropped_without_corrupting_later_bytes() {
        let mut parser = InputParser::default();

        let events = parser.process(b"a\x02?b");

        assert_eq!(events.len(), 2);
        assert_raw(&events[0], b"a");
        assert_raw(&events[1], b"b");
    }

    #[test]
    fn multiple_commands_in_one_buffer_are_all_emitted() {
        let mut parser = InputParser::default();

        let events = parser.process(b"\x02%\x02\"\x02n\x02p\x02x\x02d\x02s");

        assert_eq!(events.len(), 7);
        assert_command(&events[0], |event| matches!(event, CliEvent::SplitPaneVertical));
        assert_command(&events[1], |event| matches!(event, CliEvent::SplitPaneHorizontal));
        assert_command(&events[2], |event| matches!(event, CliEvent::NextPane));
        assert_command(&events[3], |event| matches!(event, CliEvent::PrevPane));
        assert_command(&events[4], |event| matches!(event, CliEvent::KillPane));
        assert_command(&events[5], |event| matches!(event, CliEvent::Detach));
        assert_command(&events[6], |event| matches!(event, CliEvent::OpenSessionSwitcher));
    }

    #[test]
    fn trailing_prefix_keeps_only_the_pending_prefix_buffered() {
        let mut parser = InputParser::default();

        let first = parser.process(b"ok\x02");
        let second = parser.process(b"p");

        assert_eq!(first.len(), 1);
        assert_raw(&first[0], b"ok");
        assert_eq!(second.len(), 1);
        assert_command(&second[0], |event| matches!(event, CliEvent::PrevPane));
    }
}

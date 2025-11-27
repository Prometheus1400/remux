use remux_core::events::CliEvent;

use crate::{
    input_parser::events::{Action, ParsedEvent},
    prelude::*,
};

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

#[derive(Debug)]
pub struct InputParser {
    buf: Vec<u8>,
}

impl InputParser {
    pub fn new() -> Self {
        Self { buf: vec![] }
    }

    pub fn process(&mut self, input: &[u8]) -> Vec<ParsedEvent> {
        use ParsedEvent::{DaemonAction, LocalAction};
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
                            events.push(DaemonAction(CliEvent::Raw(old)));
                            i = 0;
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
                                events.push(LocalAction(Action::SwitchSession));
                                self.buf.drain(..2);
                            }
                            _ => {
                                self.buf.drain(..=i);
                            }
                        }
                        i = 0;
                    } else {
                        let old: Vec<u8> = self.buf.drain(..i).collect();
                        if !old.is_empty() {
                            events.push(DaemonAction(CliEvent::Raw(old)));
                        }
                        break;
                    }
                }
                _ => i += 1,
            }
        }

        let old: Vec<u8> = self.buf.drain(..i).collect();
        if !old.is_empty() {
            events.push(DaemonAction(CliEvent::Raw(old)));
        }
        trace!("return from process with remaining {:?}", self.buf);
        events
    }
}

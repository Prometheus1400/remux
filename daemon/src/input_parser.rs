use bytes::Bytes;

use crate::prelude::*;

const CTRL_SPACE: u8 = 0x00;
const CTRL_B: u8 = 0x02;
const PERCENT: u8 = 0x25;
const DOUBLE_QUOTE: u8 = 0x22;
const N: u8 = 0x6E;
const P: u8 = 0x70;
const S: u8 = 0x73;
const X: u8 = 0x78;

pub enum ParsedEvents {
    Raw(Bytes),
    KillPane,
    NextPane,
    SplitPaneVertical,
    SplitPaneHorizontal,
    PrevPane,
    RequestSwitchSession, // trigger for UI switch session popup
}

pub struct InputParser {
    buf: Vec<u8>,
}

impl InputParser {
    pub fn new() -> Self {
        Self { buf: vec![] }
    }

    pub fn process(&mut self, input: &[u8]) -> Vec<ParsedEvents> {
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
                            events.push(ParsedEvents::Raw(Bytes::from(old)));
                            i = 0;
                        }
                        match b_next {
                            PERCENT => {
                                events.push(ParsedEvents::SplitPaneVertical);
                                self.buf.drain(..2);
                            }
                            DOUBLE_QUOTE => {
                                events.push(ParsedEvents::SplitPaneHorizontal);
                                self.buf.drain(..2);
                            }
                            N => {
                                events.push(ParsedEvents::NextPane);
                                self.buf.drain(..2);
                            }
                            P => {
                                events.push(ParsedEvents::PrevPane);
                                self.buf.drain(..2);
                            }
                            S => {
                                events.push(ParsedEvents::RequestSwitchSession);
                                self.buf.drain(..2);
                            }
                            X => {
                                events.push(ParsedEvents::KillPane);
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
                            events.push(ParsedEvents::Raw(Bytes::from(old)));
                        }
                        break;
                    }
                }
                _ => i += 1,
            }
        }

        let old: Vec<u8> = self.buf.drain(..i).collect();
        if !old.is_empty() {
            events.push(ParsedEvents::Raw(Bytes::from(old)));
        }
        trace!("return from process with remaining {:?}", self.buf);
        events
    }
}

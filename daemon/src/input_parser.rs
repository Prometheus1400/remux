use bytes::Bytes;
use crate::prelude::*;


const CTRL_SPACE: u8 = 0x00;
const PERCENT: u8 = 0x25;
const X: u8 = 0x78;

pub enum ParsedEvents {
    Raw(Bytes),
    KillPane,
    SplitPane,
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
        let mut p = 0;
        let mut i = 0;
        while i < self.buf.len() {
            let b = self.buf[i];

            match b {
                CTRL_SPACE => {
                    debug!("prefix detected");
                    if (i + 1) < self.buf.len() {
                        let b_next = self.buf[i + 1];
                        match b_next {
                            PERCENT => {
                                let old: Vec<u8> = self.buf.drain(p..i).collect();
                                if !old.is_empty() {
                                    events.push(ParsedEvents::Raw(Bytes::from(old)));
                                }
                                events.push(ParsedEvents::SplitPane);
                                i += 2;
                                p = i;
                            },
                            X => {
                                let old: Vec<u8> = self.buf.drain(p..i).collect();
                                if !old.is_empty() {
                                    events.push(ParsedEvents::Raw(Bytes::from(old)));
                                }
                                events.push(ParsedEvents::KillPane);
                                i += 2;
                                p = i;
                            }
                            _ => {
                                let old: Vec<u8> = self.buf.drain(p..i).collect();
                                if !old.is_empty() {
                                    events.push(ParsedEvents::Raw(Bytes::from(old)));
                                }
                                i += 1;
                                p = i;
                            }
                        }
                    } else {
                        let old: Vec<u8> = self.buf.drain(p..i).collect();
                        if !old.is_empty() {
                            events.push(ParsedEvents::Raw(Bytes::from(old)));
                        }
                        return events;
                    }
                }, 
                _ => {
                    i += 1
                }
            }
        }

        let old: Vec<u8> = self.buf.drain(p..i).collect();
        if !old.is_empty() {
            events.push(ParsedEvents::Raw(Bytes::from(old)));
        }
        events
    }
}

use bytes::Bytes;
use remux_core::config::{BuiltinAction, KeyAction, KeyBinding};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedInput {
    Raw(Bytes),
    Builtin(BuiltinAction),
    Named(String),
}

#[derive(Debug, Default)]
pub struct InputParser {
    buf: Vec<u8>,
    key_bindings: Vec<KeyBinding>,
}

impl InputParser {
    pub fn new(key_bindings: &[KeyBinding]) -> Self {
        Self {
            buf: Vec::new(),
            key_bindings: key_bindings.to_vec(),
        }
    }

    pub fn process(&mut self, input: &[u8]) -> Vec<ParsedInput> {
        self.buf.extend(input);
        let mut events = Vec::new();

        loop {
            let Some(outcome) = self.next_outcome() else {
                break;
            };

            match outcome {
                ParseOutcome::EmitRaw(bytes) => events.push(ParsedInput::Raw(Bytes::from(bytes))),
                ParseOutcome::EmitAction(action) => events.push(binding_to_input(action)),
                ParseOutcome::Pending => break,
            }
        }

        if !self.buf.is_empty() && !self.has_pending_prefix() {
            let bytes = std::mem::take(&mut self.buf);
            events.push(ParsedInput::Raw(Bytes::from(bytes)));
        }

        events
    }

    fn next_outcome(&mut self) -> Option<ParseOutcome> {
        if self.buf.is_empty() {
            return None;
        }

        if let Some(binding_index) = self.longest_exact_match_index() {
            let binding = self.key_bindings[binding_index].clone();
            let sequence_len = binding.sequence.len();
            self.buf.drain(..sequence_len);
            return Some(ParseOutcome::EmitAction(binding.action));
        }

        if self.has_pending_prefix() {
            return Some(ParseOutcome::Pending);
        }

        let next_start = self.next_binding_start_index().unwrap_or(self.buf.len());
        let bytes: Vec<u8> = self.buf.drain(..next_start.max(1)).collect();
        Some(ParseOutcome::EmitRaw(bytes))
    }

    fn longest_exact_match_index(&self) -> Option<usize> {
        self.key_bindings
            .iter()
            .enumerate()
            .filter(|(_, binding)| self.buf.starts_with(&binding.sequence))
            .max_by_key(|(_, binding)| binding.sequence.len())
            .map(|(index, _)| index)
    }

    fn has_pending_prefix(&self) -> bool {
        self.key_bindings
            .iter()
            .any(|binding| binding.sequence.len() > self.buf.len() && binding.sequence.starts_with(&self.buf))
    }

    fn next_binding_start_index(&self) -> Option<usize> {
        self.buf.iter().enumerate().skip(1).find_map(|(index, byte)| {
            self.key_bindings
                .iter()
                .any(|binding| binding.sequence.first().copied() == Some(*byte))
                .then_some(index)
        })
    }
}

enum ParseOutcome {
    EmitRaw(Vec<u8>),
    EmitAction(KeyAction),
    Pending,
}

fn binding_to_input(action: KeyAction) -> ParsedInput {
    match action {
        KeyAction::Builtin(action) => ParsedInput::Builtin(action),
        KeyAction::Named(name) => ParsedInput::Named(name),
    }
}

#[cfg(test)]
mod tests {
    use remux_core::config::{BuiltinAction, KeyAction, KeyBinding};

    use super::{InputParser, ParsedInput};

    fn test_bindings() -> Vec<KeyBinding> {
        vec![
            KeyBinding {
                sequence: b"\x02%".to_vec(),
                action: KeyAction::Builtin(BuiltinAction::SplitPaneVertical),
            },
            KeyBinding {
                sequence: b"\x02\"".to_vec(),
                action: KeyAction::Builtin(BuiltinAction::SplitPaneHorizontal),
            },
            KeyBinding {
                sequence: b"\x02n".to_vec(),
                action: KeyAction::Builtin(BuiltinAction::FocusPaneRight),
            },
            KeyBinding {
                sequence: b"\x02p".to_vec(),
                action: KeyAction::Builtin(BuiltinAction::FocusPaneLeft),
            },
            KeyBinding {
                sequence: b"\x02x".to_vec(),
                action: KeyAction::Builtin(BuiltinAction::KillPane),
            },
            KeyBinding {
                sequence: b"\x02d".to_vec(),
                action: KeyAction::Builtin(BuiltinAction::Detach),
            },
            KeyBinding {
                sequence: b"\x02s".to_vec(),
                action: KeyAction::Builtin(BuiltinAction::OpenSessionSwitcher),
            },
            KeyBinding {
                sequence: b"\x02a".to_vec(),
                action: KeyAction::Named("combo".to_owned()),
            },
        ]
    }

    fn assert_raw(event: &ParsedInput, expected: &[u8]) {
        match event {
            ParsedInput::Raw(bytes) => assert_eq!(bytes.as_ref(), expected),
            other => panic!("expected raw event, got {other:?}"),
        }
    }

    #[test]
    fn raw_input_passes_through_without_prefix() {
        let mut parser = InputParser::new(&test_bindings());

        let events = parser.process(b"hello");

        assert_eq!(events.len(), 1);
        assert_raw(&events[0], b"hello");
    }

    #[test]
    fn prefix_split_across_calls_is_buffered() {
        let mut parser = InputParser::new(&test_bindings());

        let first = parser.process(&[0x02]);
        let second = parser.process(b"n");

        assert!(first.is_empty());
        assert_eq!(second, vec![ParsedInput::Builtin(BuiltinAction::FocusPaneRight)]);
    }

    #[test]
    fn raw_before_prefix_is_emitted_before_command() {
        let mut parser = InputParser::new(&test_bindings());

        let events = parser.process(b"ab\x02d");

        assert_eq!(events.len(), 2);
        assert_raw(&events[0], b"ab");
        assert_eq!(events[1], ParsedInput::Builtin(BuiltinAction::Detach));
    }

    #[test]
    fn unknown_prefix_command_is_dropped_without_corrupting_later_bytes() {
        let mut parser = InputParser::new(&test_bindings());

        let events = parser.process(b"a\x02?b");

        assert_eq!(events.len(), 2);
        assert_raw(&events[0], b"a");
        assert_raw(&events[1], b"\x02?b");
    }

    #[test]
    fn multiple_commands_in_one_buffer_are_all_emitted() {
        let mut parser = InputParser::new(&test_bindings());

        let events = parser.process(b"\x02%\x02\"\x02n\x02p\x02x\x02d\x02s");

        assert_eq!(
            events,
            vec![
                ParsedInput::Builtin(BuiltinAction::SplitPaneVertical),
                ParsedInput::Builtin(BuiltinAction::SplitPaneHorizontal),
                ParsedInput::Builtin(BuiltinAction::FocusPaneRight),
                ParsedInput::Builtin(BuiltinAction::FocusPaneLeft),
                ParsedInput::Builtin(BuiltinAction::KillPane),
                ParsedInput::Builtin(BuiltinAction::Detach),
                ParsedInput::Builtin(BuiltinAction::OpenSessionSwitcher),
            ]
        );
    }

    #[test]
    fn trailing_prefix_keeps_only_the_pending_prefix_buffered() {
        let mut parser = InputParser::new(&test_bindings());

        let first = parser.process(b"ok\x02");
        let second = parser.process(b"p");

        assert_eq!(first.len(), 1);
        assert_raw(&first[0], b"ok");
        assert_eq!(second, vec![ParsedInput::Builtin(BuiltinAction::FocusPaneLeft)]);
    }

    #[test]
    fn named_binding_emits_named_action_event() {
        let mut parser = InputParser::new(&test_bindings());

        let events = parser.process(b"\x02a");

        assert_eq!(events, vec![ParsedInput::Named("combo".to_owned())]);
    }
}

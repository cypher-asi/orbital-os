//! Terminal Protocol
//!
//! Serialization for terminal state and input (Backend ↔ UI).

use super::wire::{decode_string, decode_u32, encode_string, Envelope};
use crate::error::ProtocolError;
use alloc::string::String;
use alloc::vec::Vec;

/// Type tag for terminal state
pub const TYPE_TERMINAL_STATE: u8 = 0x03;

/// Type tag for terminal input
pub const TYPE_TERMINAL_INPUT: u8 = 0x20;

/// Input action types
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum InputAction {
    /// Regular character input
    Char = 0,
    /// Enter key pressed (execute command)
    Enter = 1,
    /// Tab key pressed (autocomplete)
    Tab = 2,
    /// Up arrow (history previous)
    Up = 3,
    /// Down arrow (history next)
    Down = 4,
    /// Backspace
    Backspace = 5,
    /// Delete
    Delete = 6,
    /// Ctrl+C (interrupt)
    Interrupt = 7,
    /// Ctrl+D (EOF)
    Eof = 8,
    /// Ctrl+L (clear)
    Clear = 9,
}

impl InputAction {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(InputAction::Char),
            1 => Some(InputAction::Enter),
            2 => Some(InputAction::Tab),
            3 => Some(InputAction::Up),
            4 => Some(InputAction::Down),
            5 => Some(InputAction::Backspace),
            6 => Some(InputAction::Delete),
            7 => Some(InputAction::Interrupt),
            8 => Some(InputAction::Eof),
            9 => Some(InputAction::Clear),
            _ => None,
        }
    }
}

/// Terminal state sent from backend to UI
#[derive(Clone, Debug, Default)]
pub struct TerminalState {
    /// Output text to display
    pub output: String,
    /// Current prompt string
    pub prompt: String,
    /// Cursor position in current input line
    pub cursor_pos: u32,
    /// Whether terminal is ready for input
    pub ready: bool,
}

impl TerminalState {
    /// Create a new terminal state
    pub fn new(output: String, prompt: String, cursor_pos: u32, ready: bool) -> Self {
        Self {
            output,
            prompt,
            cursor_pos,
            ready,
        }
    }

    /// Create state with just output (appends to terminal)
    pub fn output_only(output: String) -> Self {
        Self {
            output,
            prompt: String::new(),
            cursor_pos: 0,
            ready: true,
        }
    }

    /// Create state with prompt (ready for input)
    pub fn with_prompt(prompt: String) -> Self {
        Self {
            output: String::new(),
            prompt,
            cursor_pos: 0,
            ready: true,
        }
    }

    /// Serialize to bytes (for sending via IPC)
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.push(TYPE_TERMINAL_STATE);
        payload.extend_from_slice(&encode_string(&self.output));
        payload.extend_from_slice(&encode_string(&self.prompt));
        payload.extend_from_slice(&self.cursor_pos.to_le_bytes());
        payload.push(if self.ready { 1 } else { 0 });

        let envelope = Envelope::new(TYPE_TERMINAL_STATE, payload);
        super::wire::encode_envelope(&envelope)
    }

    /// Deserialize from bytes (received via IPC)
    pub fn from_bytes(data: &[u8]) -> Result<Self, ProtocolError> {
        let envelope = super::wire::decode_envelope(data)?;

        if envelope.type_tag != TYPE_TERMINAL_STATE {
            return Err(ProtocolError::UnknownMessageType(envelope.type_tag));
        }

        let payload = &envelope.payload;
        if payload.is_empty() {
            return Err(ProtocolError::EmptyPayload);
        }

        // Skip the type tag in payload
        let mut cursor = 1;

        let output = decode_string(payload, &mut cursor)?;
        let prompt = decode_string(payload, &mut cursor)?;
        let cursor_pos = decode_u32(payload, &mut cursor)?;
        let ready = if cursor < payload.len() {
            payload[cursor] != 0
        } else {
            true
        };

        Ok(Self {
            output,
            prompt,
            cursor_pos,
            ready,
        })
    }
}

/// Terminal input sent from UI to backend
#[derive(Clone, Debug)]
pub struct TerminalInput {
    /// Input text (for Char action, this is the character(s); for Enter, the full line)
    pub text: String,
    /// The action type
    pub action: InputAction,
}

impl TerminalInput {
    /// Create a character input
    pub fn char(c: char) -> Self {
        Self {
            text: c.to_string(),
            action: InputAction::Char,
        }
    }

    /// Create an enter (execute) input
    pub fn enter(line: String) -> Self {
        Self {
            text: line,
            action: InputAction::Enter,
        }
    }

    /// Create a special action input
    pub fn action(action: InputAction) -> Self {
        Self {
            text: String::new(),
            action,
        }
    }

    /// Serialize to bytes (for sending via IPC)
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.push(TYPE_TERMINAL_INPUT);
        payload.push(self.action as u8);
        payload.extend_from_slice(&encode_string(&self.text));

        let envelope = Envelope::new(TYPE_TERMINAL_INPUT, payload);
        super::wire::encode_envelope(&envelope)
    }

    /// Deserialize from bytes (received via IPC)
    pub fn from_bytes(data: &[u8]) -> Result<Self, ProtocolError> {
        let envelope = super::wire::decode_envelope(data)?;

        if envelope.type_tag != TYPE_TERMINAL_INPUT {
            return Err(ProtocolError::UnknownMessageType(envelope.type_tag));
        }

        let payload = &envelope.payload;
        if payload.len() < 2 {
            return Err(ProtocolError::TooShort);
        }

        // Skip type tag
        let mut cursor = 1;

        let action = InputAction::from_u8(payload[cursor])
            .ok_or(ProtocolError::UnknownMessageType(payload[cursor]))?;
        cursor += 1;

        let text = decode_string(payload, &mut cursor)?;

        Ok(Self { text, action })
    }
}

/// Console input message tag (from supervisor)
/// This is the tag used when supervisor sends raw console input to terminal.
/// Re-exported from zos-ipc via zos-process.
pub use zos_process::MSG_CONSOLE_INPUT;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_state_roundtrip() {
        let state = TerminalState::new(
            String::from("Hello, World!\n"),
            String::from("zero> "),
            0,
            true,
        );
        let bytes = state.to_bytes();
        let decoded = TerminalState::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.output, "Hello, World!\n");
        assert_eq!(decoded.prompt, "zero> ");
        assert_eq!(decoded.cursor_pos, 0);
        assert!(decoded.ready);
    }

    #[test]
    fn test_terminal_input_char_roundtrip() {
        let input = TerminalInput::char('x');
        let bytes = input.to_bytes();
        let decoded = TerminalInput::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.text, "x");
        assert_eq!(decoded.action, InputAction::Char);
    }

    #[test]
    fn test_terminal_input_enter_roundtrip() {
        let input = TerminalInput::enter(String::from("ps"));
        let bytes = input.to_bytes();
        let decoded = TerminalInput::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.text, "ps");
        assert_eq!(decoded.action, InputAction::Enter);
    }

    #[test]
    fn test_terminal_input_action_roundtrip() {
        let input = TerminalInput::action(InputAction::Tab);
        let bytes = input.to_bytes();
        let decoded = TerminalInput::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.text, "");
        assert_eq!(decoded.action, InputAction::Tab);
    }
}

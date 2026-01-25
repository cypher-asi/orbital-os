//! Input Event Protocol
//!
//! Serialization for user input events (UI → App).

use super::type_tags::{TYPE_BUTTON_PRESS, TYPE_FOCUS_CHANGE, TYPE_KEY_PRESS, TYPE_TEXT_INPUT};
use super::wire::{decode_string, decode_u32, decode_u8, encode_string, Envelope};
use crate::error::ProtocolError;
use alloc::string::String;
use alloc::vec::Vec;

/// Abstract input event (UI converts platform events to these)
#[derive(Clone, Debug)]
pub enum InputEvent {
    /// A named button was pressed
    ButtonPress {
        /// Button identifier (e.g., "digit_5", "op_add", "clear")
        name: String,
    },

    /// Text was entered (for text input fields)
    TextInput {
        /// The entered text
        text: String,
    },

    /// A key was pressed
    KeyPress {
        /// Standard key code
        key_code: u32,
        /// Modifier flags (shift=1, ctrl=2, alt=4)
        modifiers: u8,
    },

    /// Focus gained/lost
    FocusChange {
        /// True if focus was gained, false if lost
        gained: bool,
    },
}

impl InputEvent {
    /// Create a button press event
    pub fn button(name: impl Into<String>) -> Self {
        InputEvent::ButtonPress { name: name.into() }
    }

    /// Create a text input event
    pub fn text(text: impl Into<String>) -> Self {
        InputEvent::TextInput { text: text.into() }
    }

    /// Create a key press event
    pub fn key(key_code: u32, modifiers: u8) -> Self {
        InputEvent::KeyPress {
            key_code,
            modifiers,
        }
    }

    /// Create a focus change event
    pub fn focus(gained: bool) -> Self {
        InputEvent::FocusChange { gained }
    }

    /// Serialize to bytes (for sending via IPC)
    pub fn to_bytes(&self) -> Vec<u8> {
        let (type_tag, payload) = match self {
            InputEvent::ButtonPress { name } => {
                let mut payload = Vec::new();
                payload.push(TYPE_BUTTON_PRESS);
                payload.extend_from_slice(&encode_string(name));
                (TYPE_BUTTON_PRESS, payload)
            }
            InputEvent::TextInput { text } => {
                let mut payload = Vec::new();
                payload.push(TYPE_TEXT_INPUT);
                payload.extend_from_slice(&encode_string(text));
                (TYPE_TEXT_INPUT, payload)
            }
            InputEvent::KeyPress {
                key_code,
                modifiers,
            } => {
                let mut payload = Vec::new();
                payload.push(TYPE_KEY_PRESS);
                payload.extend_from_slice(&key_code.to_le_bytes());
                payload.push(*modifiers);
                (TYPE_KEY_PRESS, payload)
            }
            InputEvent::FocusChange { gained } => {
                let mut payload = Vec::new();
                payload.push(TYPE_FOCUS_CHANGE);
                payload.push(if *gained { 1 } else { 0 });
                (TYPE_FOCUS_CHANGE, payload)
            }
        };

        let envelope = Envelope::new(type_tag, payload);
        super::wire::encode_envelope(&envelope)
    }

    /// Deserialize from bytes (received via IPC)
    pub fn from_bytes(data: &[u8]) -> Result<Self, ProtocolError> {
        // Decode envelope
        let envelope = super::wire::decode_envelope(data)?;

        let payload = &envelope.payload;
        if payload.is_empty() {
            return Err(ProtocolError::EmptyPayload);
        }

        // First byte in payload is also the type tag
        let type_tag = payload[0];
        let mut cursor = 1;

        match type_tag {
            TYPE_BUTTON_PRESS => {
                let name = decode_string(payload, &mut cursor)?;
                Ok(InputEvent::ButtonPress { name })
            }
            TYPE_TEXT_INPUT => {
                let text = decode_string(payload, &mut cursor)?;
                Ok(InputEvent::TextInput { text })
            }
            TYPE_KEY_PRESS => {
                let key_code = decode_u32(payload, &mut cursor)?;
                let modifiers = decode_u8(payload, &mut cursor)?;
                Ok(InputEvent::KeyPress {
                    key_code,
                    modifiers,
                })
            }
            TYPE_FOCUS_CHANGE => {
                let gained = decode_u8(payload, &mut cursor)? != 0;
                Ok(InputEvent::FocusChange { gained })
            }
            _ => Err(ProtocolError::UnknownMessageType(type_tag)),
        }
    }

    /// Check if this is a button press with the given name
    pub fn is_button(&self, expected_name: &str) -> bool {
        matches!(self, InputEvent::ButtonPress { name } if name == expected_name)
    }

    /// Get the button name if this is a button press
    pub fn button_name(&self) -> Option<&str> {
        match self {
            InputEvent::ButtonPress { name } => Some(name),
            _ => None,
        }
    }

    /// Get the text content if this is a text input event
    pub fn text_content(&self) -> Option<&str> {
        match self {
            InputEvent::TextInput { text } => Some(text),
            _ => None,
        }
    }
}

/// Modifier key flags
#[allow(dead_code)]
pub mod modifiers {
    pub const SHIFT: u8 = 1;
    pub const CTRL: u8 = 2;
    pub const ALT: u8 = 4;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_button_press_roundtrip() {
        let event = InputEvent::button("digit_5");
        let bytes = event.to_bytes();
        let decoded = InputEvent::from_bytes(&bytes).unwrap();

        match decoded {
            InputEvent::ButtonPress { name } => assert_eq!(name, "digit_5"),
            _ => panic!("Expected ButtonPress"),
        }
    }

    #[test]
    fn test_text_input_roundtrip() {
        let event = InputEvent::text("Hello, World!");
        let bytes = event.to_bytes();
        let decoded = InputEvent::from_bytes(&bytes).unwrap();

        match decoded {
            InputEvent::TextInput { text } => assert_eq!(text, "Hello, World!"),
            _ => panic!("Expected TextInput"),
        }
    }

    #[test]
    fn test_key_press_roundtrip() {
        let event = InputEvent::key(65, modifiers::SHIFT | modifiers::CTRL);
        let bytes = event.to_bytes();
        let decoded = InputEvent::from_bytes(&bytes).unwrap();

        match decoded {
            InputEvent::KeyPress {
                key_code,
                modifiers,
            } => {
                assert_eq!(key_code, 65);
                assert_eq!(modifiers, 3); // SHIFT | CTRL
            }
            _ => panic!("Expected KeyPress"),
        }
    }

    #[test]
    fn test_focus_change_roundtrip() {
        let event = InputEvent::focus(true);
        let bytes = event.to_bytes();
        let decoded = InputEvent::from_bytes(&bytes).unwrap();

        match decoded {
            InputEvent::FocusChange { gained } => assert!(gained),
            _ => panic!("Expected FocusChange"),
        }
    }

    #[test]
    fn test_is_button() {
        let event = InputEvent::button("op_add");
        assert!(event.is_button("op_add"));
        assert!(!event.is_button("op_sub"));
    }
}

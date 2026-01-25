//! Calculator State Protocol
//!
//! Serialization for Calculator app state.

use super::type_tags::TYPE_CALCULATOR_STATE;
use super::wire::{
    decode_optional_char, decode_string, decode_u8, encode_optional_char, encode_string, Envelope,
};
use crate::error::ProtocolError;
use alloc::string::String;
use alloc::vec::Vec;

/// Calculator app state - sent via MSG_APP_STATE
#[derive(Clone, Debug, Default)]
pub struct CalculatorState {
    /// Current display value
    pub display: String,

    /// Pending operation indicator (e.g., '+', '-', '×', '÷')
    pub pending_op: Option<char>,

    /// Whether an error occurred (e.g., division by zero)
    pub has_error: bool,

    /// Memory indicator (true if memory is set)
    pub memory_indicator: bool,
}

impl CalculatorState {
    /// Create a new CalculatorState
    pub fn new(
        display: String,
        pending_op: Option<char>,
        has_error: bool,
        memory_indicator: bool,
    ) -> Self {
        Self {
            display,
            pending_op,
            has_error,
            memory_indicator,
        }
    }

    /// Create an initial state showing "0"
    pub fn initial() -> Self {
        Self {
            display: String::from("0"),
            pending_op: None,
            has_error: false,
            memory_indicator: false,
        }
    }

    /// Create an error state
    pub fn error() -> Self {
        Self {
            display: String::from("Error"),
            pending_op: None,
            has_error: true,
            memory_indicator: false,
        }
    }

    /// Serialize to bytes (for sending via IPC)
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut payload = Vec::new();

        // Type tag
        payload.push(TYPE_CALCULATOR_STATE);

        // display (length-prefixed string)
        payload.extend_from_slice(&encode_string(&self.display));

        // pending_op (optional char)
        payload.extend_from_slice(&encode_optional_char(self.pending_op));

        // has_error (u8 bool)
        payload.push(if self.has_error { 1 } else { 0 });

        // memory_indicator (u8 bool)
        payload.push(if self.memory_indicator { 1 } else { 0 });

        // Wrap in envelope
        let envelope = Envelope::new(TYPE_CALCULATOR_STATE, payload);
        super::wire::encode_envelope(&envelope)
    }

    /// Deserialize from bytes (received via IPC)
    pub fn from_bytes(data: &[u8]) -> Result<Self, ProtocolError> {
        // Decode envelope
        let envelope = super::wire::decode_envelope(data)?;

        // Check type tag
        if envelope.type_tag != TYPE_CALCULATOR_STATE {
            return Err(ProtocolError::UnexpectedType {
                expected: TYPE_CALCULATOR_STATE,
                got: envelope.type_tag,
            });
        }

        let payload = &envelope.payload;
        if payload.is_empty() {
            return Err(ProtocolError::EmptyPayload);
        }

        // Skip type tag in payload (already checked via envelope)
        let mut cursor = 1;

        // Parse fields
        let display = decode_string(payload, &mut cursor)?;
        let pending_op = decode_optional_char(payload, &mut cursor)?;
        let has_error = decode_u8(payload, &mut cursor)? != 0;
        let memory_indicator = decode_u8(payload, &mut cursor)? != 0;

        Ok(CalculatorState {
            display,
            pending_op,
            has_error,
            memory_indicator,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculator_state_roundtrip() {
        let state = CalculatorState {
            display: String::from("123.456"),
            pending_op: Some('+'),
            has_error: false,
            memory_indicator: true,
        };

        let bytes = state.to_bytes();
        let decoded = CalculatorState::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.display, state.display);
        assert_eq!(decoded.pending_op, state.pending_op);
        assert_eq!(decoded.has_error, state.has_error);
        assert_eq!(decoded.memory_indicator, state.memory_indicator);
    }

    #[test]
    fn test_calculator_state_no_pending_op() {
        let state = CalculatorState {
            display: String::from("42"),
            pending_op: None,
            has_error: false,
            memory_indicator: false,
        };

        let bytes = state.to_bytes();
        let decoded = CalculatorState::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.pending_op, None);
    }

    #[test]
    fn test_calculator_state_error() {
        let state = CalculatorState::error();

        let bytes = state.to_bytes();
        let decoded = CalculatorState::from_bytes(&bytes).unwrap();

        assert!(decoded.has_error);
        assert_eq!(decoded.display, "Error");
    }

    #[test]
    fn test_calculator_state_special_ops() {
        for op in &['+', '-', '×', '÷', '*', '/'] {
            let state = CalculatorState {
                display: String::from("0"),
                pending_op: Some(*op),
                has_error: false,
                memory_indicator: false,
            };

            let bytes = state.to_bytes();
            let decoded = CalculatorState::from_bytes(&bytes).unwrap();

            assert_eq!(decoded.pending_op, Some(*op));
        }
    }
}

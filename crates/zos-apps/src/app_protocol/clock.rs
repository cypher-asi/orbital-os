//! Clock State Protocol
//!
//! Serialization for Clock app state.

use super::type_tags::TYPE_CLOCK_STATE;
use super::wire::{decode_string, decode_u8, encode_string, Envelope};
use crate::error::ProtocolError;
use alloc::string::String;
use alloc::vec::Vec;

/// Clock app state - sent via MSG_APP_STATE
#[derive(Clone, Debug, Default)]
pub struct ClockState {
    /// Formatted time string, e.g., "14:32:05"
    pub time_display: String,

    /// Formatted date string, e.g., "Wednesday, Jan 21"
    pub date_display: String,

    /// Whether 24-hour format is enabled
    pub is_24_hour: bool,

    /// Timezone name, e.g., "UTC" or "America/New_York"
    pub timezone: String,
}

impl ClockState {
    /// Create a new ClockState
    pub fn new(
        time_display: String,
        date_display: String,
        is_24_hour: bool,
        timezone: String,
    ) -> Self {
        Self {
            time_display,
            date_display,
            is_24_hour,
            timezone,
        }
    }

    /// Serialize to bytes (for sending via IPC)
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut payload = Vec::new();

        // Type tag
        payload.push(TYPE_CLOCK_STATE);

        // time_display (length-prefixed string)
        payload.extend_from_slice(&encode_string(&self.time_display));

        // date_display (length-prefixed string)
        payload.extend_from_slice(&encode_string(&self.date_display));

        // is_24_hour (u8 bool)
        payload.push(if self.is_24_hour { 1 } else { 0 });

        // timezone (length-prefixed string)
        payload.extend_from_slice(&encode_string(&self.timezone));

        // Wrap in envelope
        let envelope = Envelope::new(TYPE_CLOCK_STATE, payload);
        super::wire::encode_envelope(&envelope)
    }

    /// Deserialize from bytes (received via IPC)
    pub fn from_bytes(data: &[u8]) -> Result<Self, ProtocolError> {
        // Decode envelope
        let envelope = super::wire::decode_envelope(data)?;

        // Check type tag
        if envelope.type_tag != TYPE_CLOCK_STATE {
            return Err(ProtocolError::UnexpectedType {
                expected: TYPE_CLOCK_STATE,
                got: envelope.type_tag,
            });
        }

        let payload = &envelope.payload;
        if payload.is_empty() {
            return Err(ProtocolError::EmptyPayload);
        }

        // Skip type tag in payload (already checked via envelope)
        let mut cursor = 1;

        // Parse strings
        let time_display = decode_string(payload, &mut cursor)?;
        let date_display = decode_string(payload, &mut cursor)?;
        let is_24_hour = decode_u8(payload, &mut cursor)? != 0;
        let timezone = decode_string(payload, &mut cursor)?;

        Ok(ClockState {
            time_display,
            date_display,
            is_24_hour,
            timezone,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clock_state_roundtrip() {
        let state = ClockState {
            time_display: String::from("14:32:05"),
            date_display: String::from("Wednesday, Jan 21"),
            is_24_hour: true,
            timezone: String::from("UTC"),
        };

        let bytes = state.to_bytes();
        let decoded = ClockState::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.time_display, state.time_display);
        assert_eq!(decoded.date_display, state.date_display);
        assert_eq!(decoded.is_24_hour, state.is_24_hour);
        assert_eq!(decoded.timezone, state.timezone);
    }

    #[test]
    fn test_clock_state_12_hour() {
        let state = ClockState {
            time_display: String::from("2:32:05 PM"),
            date_display: String::from("Wed, Jan 21"),
            is_24_hour: false,
            timezone: String::from("America/New_York"),
        };

        let bytes = state.to_bytes();
        let decoded = ClockState::from_bytes(&bytes).unwrap();

        assert!(!decoded.is_24_hour);
    }
}

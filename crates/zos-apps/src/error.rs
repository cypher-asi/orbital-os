//! Error Types for Zero OS Apps
//!
//! Defines errors that can occur during app execution.

use crate::manifest::ObjectType;
use alloc::string::String;
use core::fmt;

/// Errors that can occur in app execution
#[derive(Clone, Debug)]
pub enum AppError {
    /// Initialization failed
    InitFailed(String),

    /// Required capability not granted
    MissingCapability(ObjectType),

    /// IPC communication error
    IpcError(String),

    /// Protocol error (invalid message format)
    ProtocolError(ProtocolError),

    /// Internal application error
    Internal(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::InitFailed(msg) => write!(f, "Initialization failed: {}", msg),
            AppError::MissingCapability(obj_type) => {
                write!(f, "Missing capability: {:?}", obj_type)
            }
            AppError::IpcError(msg) => write!(f, "IPC error: {}", msg),
            AppError::ProtocolError(e) => write!(f, "Protocol error: {}", e),
            AppError::Internal(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl From<ProtocolError> for AppError {
    fn from(e: ProtocolError) -> Self {
        AppError::ProtocolError(e)
    }
}

/// Errors that can occur during protocol parsing
#[derive(Clone, Debug)]
pub enum ProtocolError {
    /// Message is too short to contain required fields
    TooShort,

    /// Unknown protocol version
    UnknownVersion(u8),

    /// Payload length exceeds available data
    PayloadOverflow { declared: usize, available: usize },

    /// String length exceeds available data
    StringOverflow { declared: usize, available: usize },

    /// Invalid UTF-8 in string
    InvalidUtf8,

    /// Empty payload
    EmptyPayload,

    /// Unexpected type tag
    UnexpectedType { expected: u8, got: u8 },

    /// Unknown message type
    UnknownMessageType(u8),

    /// Invalid enum value
    InvalidEnumValue { field: &'static str, value: u8 },
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolError::TooShort => write!(f, "Message too short"),
            ProtocolError::UnknownVersion(v) => write!(f, "Unknown protocol version: {}", v),
            ProtocolError::PayloadOverflow {
                declared,
                available,
            } => {
                write!(
                    f,
                    "Payload overflow: declared {} bytes, only {} available",
                    declared, available
                )
            }
            ProtocolError::StringOverflow {
                declared,
                available,
            } => {
                write!(
                    f,
                    "String overflow: declared {} bytes, only {} available",
                    declared, available
                )
            }
            ProtocolError::InvalidUtf8 => write!(f, "Invalid UTF-8 in string"),
            ProtocolError::EmptyPayload => write!(f, "Empty payload"),
            ProtocolError::UnexpectedType { expected, got } => {
                write!(
                    f,
                    "Unexpected type: expected 0x{:02x}, got 0x{:02x}",
                    expected, got
                )
            }
            ProtocolError::UnknownMessageType(t) => write!(f, "Unknown message type: 0x{:02x}", t),
            ProtocolError::InvalidEnumValue { field, value } => {
                write!(f, "Invalid enum value for {}: {}", field, value)
            }
        }
    }
}

//! Error types for the desktop compositor
//!
//! This module provides structured error types for all fallible operations
//! in the desktop crate, following the project's error handling conventions.

use crate::desktop::DesktopId;
use crate::window::WindowId;

/// Errors that can occur in desktop compositor operations
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopError {
    /// Window with the given ID was not found
    WindowNotFound(WindowId),

    /// Desktop with the given ID was not found
    DesktopNotFound(DesktopId),

    /// Desktop at the given index was not found
    DesktopIndexOutOfBounds {
        /// The requested index
        index: usize,
        /// The actual number of desktops
        count: usize,
    },

    /// An operation was attempted that is not valid in the current state
    InvalidOperation {
        /// The operation that was attempted
        op: &'static str,
        /// Why the operation failed
        reason: &'static str,
    },

    /// JSON serialization or deserialization failed
    SerializationError(String),

    /// A render operation failed
    RenderError(String),

    /// Persistence operation failed
    PersistenceError(String),
}

impl std::fmt::Display for DesktopError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WindowNotFound(id) => write!(f, "window not found: {}", id),
            Self::DesktopNotFound(id) => write!(f, "desktop not found: {}", id),
            Self::DesktopIndexOutOfBounds { index, count } => {
                write!(
                    f,
                    "desktop index {} out of bounds (count: {})",
                    index, count
                )
            }
            Self::InvalidOperation { op, reason } => {
                write!(f, "invalid operation '{}': {}", op, reason)
            }
            Self::SerializationError(msg) => write!(f, "serialization error: {}", msg),
            Self::RenderError(msg) => write!(f, "render error: {}", msg),
            Self::PersistenceError(msg) => write!(f, "persistence error: {}", msg),
        }
    }
}

impl std::error::Error for DesktopError {}

/// Result type alias for desktop operations
pub type DesktopResult<T> = Result<T, DesktopError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = DesktopError::WindowNotFound(42);
        assert_eq!(err.to_string(), "window not found: 42");

        let err = DesktopError::DesktopNotFound(1);
        assert_eq!(err.to_string(), "desktop not found: 1");

        let err = DesktopError::DesktopIndexOutOfBounds { index: 5, count: 3 };
        assert_eq!(
            err.to_string(),
            "desktop index 5 out of bounds (count: 3)"
        );

        let err = DesktopError::InvalidOperation {
            op: "close_window",
            reason: "window is already closed",
        };
        assert_eq!(
            err.to_string(),
            "invalid operation 'close_window': window is already closed"
        );
    }

    #[test]
    fn test_error_equality() {
        let err1 = DesktopError::WindowNotFound(42);
        let err2 = DesktopError::WindowNotFound(42);
        let err3 = DesktopError::WindowNotFound(43);

        assert_eq!(err1, err2);
        assert_ne!(err1, err3);
    }
}

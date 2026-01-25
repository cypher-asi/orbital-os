//! Input result type

use crate::window::WindowId;
use serde::Serialize;

/// Result of input handling
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum InputResult {
    /// Input was handled internally
    Handled,
    /// Input was not handled (pass through)
    Unhandled,
    /// Input should be forwarded to window content
    Forward {
        /// Target window
        window_id: WindowId,
        /// X coordinate in window-local space
        local_x: f32,
        /// Y coordinate in window-local space
        local_y: f32,
    },
}

impl InputResult {
    /// Check if input was handled
    #[inline]
    pub fn is_handled(&self) -> bool {
        matches!(self, InputResult::Handled | InputResult::Forward { .. })
    }

    /// Check if input should be forwarded
    #[inline]
    pub fn is_forward(&self) -> bool {
        matches!(self, InputResult::Forward { .. })
    }
}

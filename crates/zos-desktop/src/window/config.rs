//! Window configuration for creation

use super::WindowType;
use crate::math::{Size, Vec2};

/// Configuration for creating a window
#[derive(Clone, Debug, Default)]
pub struct WindowConfig {
    /// Window title
    pub title: String,
    /// Initial position (None = auto-cascade)
    pub position: Option<Vec2>,
    /// Initial size
    pub size: Size,
    /// Minimum size constraint
    pub min_size: Option<Size>,
    /// Maximum size constraint
    pub max_size: Option<Size>,
    /// Application identifier for routing
    pub app_id: String,
    /// Associated process ID
    pub process_id: Option<u64>,
    /// Whether the window content area handles its own mouse events
    /// If false (default), clicking/dragging content will move the window
    /// If true, mouse events are forwarded to the app instead
    pub content_interactive: bool,
    /// Window type (standard or widget)
    pub window_type: WindowType,
}

//! Window struct and state

use super::WindowId;
use crate::math::{Rect, Size, Vec2, FRAME_STYLE};
use serde::{Deserialize, Serialize};

/// Window state
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowState {
    #[default]
    Normal,
    Minimized,
    Maximized,
    Fullscreen,
}

/// Window type - determines chrome/presentation style
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowType {
    /// Standard window with title bar, minimize/maximize/close buttons
    #[default]
    Standard,
    /// Widget window with no title bar, only close button
    Widget,
}

/// A window in the desktop environment
#[derive(Clone, Debug)]
pub struct Window {
    /// Unique identifier
    pub id: WindowId,
    /// Window title
    pub title: String,
    /// Application identifier (for routing)
    pub app_id: String,
    /// Position on infinite canvas (not screen)
    pub position: Vec2,
    /// Window size including frame
    pub size: Size,
    /// Minimum size
    pub min_size: Size,
    /// Maximum size (None = no limit)
    pub max_size: Option<Size>,
    /// Current state
    pub state: WindowState,
    /// Window type (standard or widget)
    pub window_type: WindowType,
    /// Associated process ID (if any)
    pub process_id: Option<u64>,
    /// Z-order (higher = on top)
    pub z_order: u32,
    /// Saved position/size for restore after maximize
    pub(crate) restore_rect: Option<(Vec2, Size)>,
    /// Previous state before minimize
    pub(crate) prev_state: Option<WindowState>,
    /// Whether the window content area handles its own mouse events
    pub content_interactive: bool,
}

impl Window {
    /// Get the window's bounding rectangle
    #[inline]
    pub fn rect(&self) -> Rect {
        Rect::from_pos_size(self.position, self.size)
    }

    /// Get the title bar rectangle
    pub fn title_bar_rect(&self) -> Rect {
        Rect::new(
            self.position.x,
            self.position.y,
            self.size.width,
            FRAME_STYLE.title_bar_height,
        )
    }

    /// Get the content area rectangle (excludes title bar)
    pub fn content_rect(&self) -> Rect {
        Rect::new(
            self.position.x,
            self.position.y + FRAME_STYLE.title_bar_height,
            self.size.width,
            self.size.height - FRAME_STYLE.title_bar_height,
        )
    }

    /// Get the close button rectangle
    pub fn close_button_rect(&self) -> Rect {
        let x =
            self.position.x + self.size.width - FRAME_STYLE.button_margin - FRAME_STYLE.button_size;
        let y = self.position.y + (FRAME_STYLE.title_bar_height - FRAME_STYLE.button_size) / 2.0;
        Rect::new(x, y, FRAME_STYLE.button_size, FRAME_STYLE.button_size)
    }

    /// Get the maximize button rectangle
    pub fn maximize_button_rect(&self) -> Rect {
        let x = self.position.x + self.size.width
            - FRAME_STYLE.button_margin
            - FRAME_STYLE.button_size * 2.0
            - FRAME_STYLE.button_spacing;
        let y = self.position.y + (FRAME_STYLE.title_bar_height - FRAME_STYLE.button_size) / 2.0;
        Rect::new(x, y, FRAME_STYLE.button_size, FRAME_STYLE.button_size)
    }

    /// Get the minimize button rectangle
    pub fn minimize_button_rect(&self) -> Rect {
        let x = self.position.x + self.size.width
            - FRAME_STYLE.button_margin
            - FRAME_STYLE.button_size * 3.0
            - FRAME_STYLE.button_spacing * 2.0;
        let y = self.position.y + (FRAME_STYLE.title_bar_height - FRAME_STYLE.button_size) / 2.0;
        Rect::new(x, y, FRAME_STYLE.button_size, FRAME_STYLE.button_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_window() -> Window {
        Window {
            id: 1,
            title: "Test".to_string(),
            app_id: "test".to_string(),
            position: Vec2::new(100.0, 100.0),
            size: Size::new(800.0, 600.0),
            min_size: Size::new(200.0, 150.0),
            max_size: None,
            state: WindowState::Normal,
            window_type: WindowType::Standard,
            process_id: None,
            z_order: 1,
            restore_rect: None,
            prev_state: None,
            content_interactive: false,
        }
    }

    #[test]
    fn test_window_rect() {
        let w = create_test_window();
        let r = w.rect();
        assert!((r.x - 100.0).abs() < 0.001);
        assert!((r.y - 100.0).abs() < 0.001);
        assert!((r.width - 800.0).abs() < 0.001);
        assert!((r.height - 600.0).abs() < 0.001);
    }

    #[test]
    fn test_window_title_bar_rect() {
        let w = create_test_window();
        let r = w.title_bar_rect();
        assert!((r.x - 100.0).abs() < 0.001);
        assert!((r.y - 100.0).abs() < 0.001);
        assert!((r.width - 800.0).abs() < 0.001);
        assert!((r.height - FRAME_STYLE.title_bar_height).abs() < 0.001);
    }

    #[test]
    fn test_window_content_rect() {
        let w = create_test_window();
        let r = w.content_rect();
        assert!((r.y - (100.0 + FRAME_STYLE.title_bar_height)).abs() < 0.001);
    }
}

//! Drag state for input operations

use crate::math::{Size, Vec2};
use crate::window::{WindowId, WindowRegion};

/// Current drag operation state
#[derive(Clone, Debug)]
pub enum DragState {
    /// Panning the canvas
    PanCanvas {
        /// Start screen position
        start: Vec2,
        /// Viewport center at start
        start_center: Vec2,
    },
    /// Moving a window
    MoveWindow {
        /// Window being moved
        window_id: WindowId,
        /// Offset from window origin to cursor
        offset: Vec2,
    },
    /// Resizing a window
    ResizeWindow {
        /// Window being resized
        window_id: WindowId,
        /// Which resize handle
        handle: WindowRegion,
        /// Window position at start
        start_pos: Vec2,
        /// Window size at start
        start_size: Size,
        /// Mouse position at start (canvas coords)
        start_mouse: Vec2,
    },
}

impl DragState {
    /// Check if this is a canvas pan operation
    #[inline]
    pub fn is_pan(&self) -> bool {
        matches!(self, DragState::PanCanvas { .. })
    }

    /// Check if this is a window move operation
    #[inline]
    pub fn is_move(&self) -> bool {
        matches!(self, DragState::MoveWindow { .. })
    }

    /// Check if this is a window resize operation
    #[inline]
    pub fn is_resize(&self) -> bool {
        matches!(self, DragState::ResizeWindow { .. })
    }

    /// Get the window ID if this is a window operation
    pub fn window_id(&self) -> Option<WindowId> {
        match self {
            DragState::MoveWindow { window_id, .. } => Some(*window_id),
            DragState::ResizeWindow { window_id, .. } => Some(*window_id),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pan_canvas_state() {
        let state = DragState::PanCanvas {
            start: Vec2::new(100.0, 100.0),
            start_center: Vec2::new(0.0, 0.0),
        };

        assert!(state.is_pan());
        assert!(!state.is_move());
        assert!(!state.is_resize());
        assert!(state.window_id().is_none());
    }

    #[test]
    fn test_move_window_state() {
        let state = DragState::MoveWindow {
            window_id: 42,
            offset: Vec2::new(10.0, 20.0),
        };

        assert!(!state.is_pan());
        assert!(state.is_move());
        assert!(!state.is_resize());
        assert_eq!(state.window_id(), Some(42));
    }

    #[test]
    fn test_resize_window_state() {
        let state = DragState::ResizeWindow {
            window_id: 123,
            handle: WindowRegion::ResizeSE,
            start_pos: Vec2::new(100.0, 100.0),
            start_size: Size::new(800.0, 600.0),
            start_mouse: Vec2::new(900.0, 700.0),
        };

        assert!(!state.is_pan());
        assert!(!state.is_move());
        assert!(state.is_resize());
        assert_eq!(state.window_id(), Some(123));
    }

    #[test]
    fn test_drag_state_clone() {
        let state = DragState::MoveWindow {
            window_id: 42,
            offset: Vec2::new(10.0, 20.0),
        };

        let cloned = state.clone();
        assert!(cloned.is_move());
        assert_eq!(cloned.window_id(), Some(42));
    }

    #[test]
    fn test_pan_canvas_preserves_start_values() {
        let state = DragState::PanCanvas {
            start: Vec2::new(500.0, 400.0),
            start_center: Vec2::new(-100.0, 200.0),
        };

        if let DragState::PanCanvas { start, start_center } = state {
            assert!((start.x - 500.0).abs() < 0.001);
            assert!((start.y - 400.0).abs() < 0.001);
            assert!((start_center.x - (-100.0)).abs() < 0.001);
            assert!((start_center.y - 200.0).abs() < 0.001);
        } else {
            panic!("Expected PanCanvas state");
        }
    }

    #[test]
    fn test_resize_window_preserves_all_fields() {
        let state = DragState::ResizeWindow {
            window_id: 99,
            handle: WindowRegion::ResizeNW,
            start_pos: Vec2::new(50.0, 75.0),
            start_size: Size::new(400.0, 300.0),
            start_mouse: Vec2::new(60.0, 85.0),
        };

        if let DragState::ResizeWindow { window_id, handle, start_pos, start_size, start_mouse } = state {
            assert_eq!(window_id, 99);
            assert_eq!(handle, WindowRegion::ResizeNW);
            assert!((start_pos.x - 50.0).abs() < 0.001);
            assert!((start_pos.y - 75.0).abs() < 0.001);
            assert!((start_size.width - 400.0).abs() < 0.001);
            assert!((start_size.height - 300.0).abs() < 0.001);
            assert!((start_mouse.x - 60.0).abs() < 0.001);
            assert!((start_mouse.y - 85.0).abs() < 0.001);
        } else {
            panic!("Expected ResizeWindow state");
        }
    }

    #[test]
    fn test_move_window_offset() {
        let state = DragState::MoveWindow {
            window_id: 1,
            offset: Vec2::new(15.5, 25.5),
        };

        if let DragState::MoveWindow { offset, .. } = state {
            assert!((offset.x - 15.5).abs() < 0.001);
            assert!((offset.y - 25.5).abs() < 0.001);
        } else {
            panic!("Expected MoveWindow state");
        }
    }
}
